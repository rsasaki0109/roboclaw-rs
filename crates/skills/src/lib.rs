use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Skill {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub resume_original_instruction: bool,
    #[serde(default)]
    pub supports_checkpoint_resume: bool,
    #[serde(default)]
    pub recovery_for: Vec<RecoveryRule>,
    #[serde(default)]
    pub steps: Vec<SkillStep>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillStep {
    pub name: String,
    pub tool: String,
    #[serde(default)]
    pub input: Value,
    #[serde(default)]
    pub expect: Value,
    #[serde(default)]
    pub max_retries: usize,
    #[serde(default)]
    pub resume_from_step: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RecoveryRule {
    #[serde(default)]
    pub failed_steps: Vec<String>,
    #[serde(default)]
    pub tools: Vec<String>,
    #[serde(default)]
    pub observation_contains: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RecoveryContext {
    pub failed_step: Option<String>,
    pub tool: Option<String>,
    pub observation: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct SkillCatalog {
    skills: BTreeMap<String, Skill>,
}

impl SkillCatalog {
    pub fn from_dir(dir: impl AsRef<Path>) -> Result<Self> {
        let dir = dir.as_ref();
        let mut files: Vec<PathBuf> = fs::read_dir(dir)
            .with_context(|| format!("failed to read skill directory {:?}", dir))?
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|path| {
                matches!(
                    path.extension().and_then(|ext| ext.to_str()),
                    Some("yml" | "yaml")
                )
            })
            .collect();

        files.sort();

        let mut skills = BTreeMap::new();
        for file in files {
            let skill = load_skill_file(&file)?;
            skills.insert(skill.name.clone(), skill);
        }

        if skills.is_empty() {
            return Err(anyhow!("no skills found in {:?}", dir));
        }

        Ok(Self { skills })
    }

    pub fn get(&self, name: &str) -> Option<&Skill> {
        self.skills.get(name)
    }

    pub fn first(&self) -> Option<&Skill> {
        self.skills.values().next()
    }

    pub fn values(&self) -> impl Iterator<Item = &Skill> {
        self.skills.values()
    }

    pub fn names(&self) -> Vec<String> {
        self.skills.keys().cloned().collect()
    }

    pub fn recovery_candidates(&self, context: &RecoveryContext) -> Vec<&Skill> {
        self.skills
            .values()
            .filter(|skill| {
                skill.resume_original_instruction && skill.matches_recovery_context(context)
            })
            .collect()
    }

    pub fn recovery_candidate_names(&self, context: &RecoveryContext) -> Vec<String> {
        self.recovery_candidates(context)
            .into_iter()
            .map(|skill| skill.name.clone())
            .collect()
    }

    pub fn recovery_skill_for_context(&self, context: &RecoveryContext) -> Option<&Skill> {
        self.recovery_candidates(context).into_iter().next()
    }
}

pub fn load_skill_file(path: impl AsRef<Path>) -> Result<Skill> {
    let path = path.as_ref();
    let content =
        fs::read_to_string(path).with_context(|| format!("failed to read skill {:?}", path))?;
    serde_yaml::from_str(&content).with_context(|| format!("failed to parse skill {:?}", path))
}

impl Skill {
    pub fn matches_recovery_context(&self, context: &RecoveryContext) -> bool {
        self.recovery_for.iter().any(|rule| rule.matches(context))
    }

    pub fn recovery_summary(&self) -> Option<String> {
        if self.recovery_for.is_empty() {
            return None;
        }

        Some(
            self.recovery_for
                .iter()
                .map(RecoveryRule::summary)
                .collect::<Vec<_>>()
                .join(" || "),
        )
    }
}

impl RecoveryRule {
    fn matches(&self, context: &RecoveryContext) -> bool {
        let has_constraints = !self.failed_steps.is_empty()
            || !self.tools.is_empty()
            || !self.observation_contains.is_empty();
        if !has_constraints {
            return false;
        }

        if !self.failed_steps.is_empty()
            && !matches_any_exact(&self.failed_steps, context.failed_step.as_deref())
        {
            return false;
        }

        if !self.tools.is_empty() && !matches_any_exact(&self.tools, context.tool.as_deref()) {
            return false;
        }

        if !self.observation_contains.is_empty()
            && !matches_any_substring(&self.observation_contains, context.observation.as_deref())
        {
            return false;
        }

        true
    }

    fn summary(&self) -> String {
        let mut parts = Vec::new();
        if !self.failed_steps.is_empty() {
            parts.push(format!("failed_steps={}", self.failed_steps.join(",")));
        }
        if !self.tools.is_empty() {
            parts.push(format!("tools={}", self.tools.join(",")));
        }
        if !self.observation_contains.is_empty() {
            parts.push(format!(
                "observation_contains={}",
                self.observation_contains.join(",")
            ));
        }

        if parts.is_empty() {
            "unspecified".to_string()
        } else {
            parts.join("; ")
        }
    }
}

fn matches_any_exact(candidates: &[String], actual: Option<&str>) -> bool {
    let Some(actual) = actual.map(|value| value.to_lowercase()) else {
        return false;
    };

    candidates
        .iter()
        .any(|candidate| candidate.to_lowercase() == actual)
}

fn matches_any_substring(candidates: &[String], actual: Option<&str>) -> bool {
    let Some(actual) = actual.map(|value| value.to_lowercase()) else {
        return false;
    };

    candidates
        .iter()
        .any(|candidate| actual.contains(&candidate.to_lowercase()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recovery_rule_matches_failed_step_and_tool() {
        let skill = Skill {
            name: "recover_grasp".to_string(),
            description: "recover grasp".to_string(),
            resume_original_instruction: true,
            supports_checkpoint_resume: false,
            recovery_for: vec![RecoveryRule {
                failed_steps: vec!["grasp".to_string()],
                tools: vec!["motor_control".to_string()],
                observation_contains: vec![],
            }],
            steps: vec![],
        };

        assert!(skill.matches_recovery_context(&RecoveryContext {
            failed_step: Some("grasp".to_string()),
            tool: Some("motor_control".to_string()),
            observation: Some("transient motor stall".to_string()),
        }));
        assert!(!skill.matches_recovery_context(&RecoveryContext {
            failed_step: Some("detect_object".to_string()),
            tool: Some("sensor".to_string()),
            observation: Some("target missing".to_string()),
        }));
    }

    #[test]
    fn catalog_returns_matching_recovery_skill_names() {
        let mut skills = BTreeMap::new();
        skills.insert(
            "recover_grasp".to_string(),
            Skill {
                name: "recover_grasp".to_string(),
                description: "recover grasp".to_string(),
                resume_original_instruction: true,
                supports_checkpoint_resume: false,
                recovery_for: vec![RecoveryRule {
                    failed_steps: vec!["grasp".to_string()],
                    tools: vec!["motor_control".to_string()],
                    observation_contains: vec![],
                }],
                steps: vec![],
            },
        );
        skills.insert(
            "recover_observation".to_string(),
            Skill {
                name: "recover_observation".to_string(),
                description: "recover sensor".to_string(),
                resume_original_instruction: true,
                supports_checkpoint_resume: false,
                recovery_for: vec![RecoveryRule {
                    failed_steps: vec![],
                    tools: vec!["sensor".to_string()],
                    observation_contains: vec![],
                }],
                steps: vec![],
            },
        );

        let catalog = SkillCatalog { skills };
        let matches = catalog.recovery_candidate_names(&RecoveryContext {
            failed_step: Some("grasp".to_string()),
            tool: Some("motor_control".to_string()),
            observation: Some("transient motor stall".to_string()),
        });

        assert_eq!(matches, vec!["recover_grasp".to_string()]);
    }
}
