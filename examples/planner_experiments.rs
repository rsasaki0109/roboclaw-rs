use anyhow::Result;
use std::env;
use std::fs;
use std::path::PathBuf;

#[path = "../experiments/artifact_mirroring_drift/mod.rs"]
mod artifact_mirroring_drift;
#[path = "../experiments/artifact_trust_decay/mod.rs"]
mod artifact_trust_decay;
#[path = "../experiments/backend_observation_fusion/mod.rs"]
mod backend_observation_fusion;
#[path = "../experiments/cross_provider_validation/mod.rs"]
mod cross_provider_validation;
#[path = "../experiments/cross_repo_provenance_stitching/mod.rs"]
mod cross_repo_provenance_stitching;
#[path = "../experiments/cross_suite_contradiction/mod.rs"]
mod cross_suite_contradiction;
#[path = "../experiments/cross_train_lag_carryover/mod.rs"]
mod cross_train_lag_carryover;
#[path = "../experiments/frontier_snapshot_replay/mod.rs"]
mod frontier_snapshot_replay;
#[path = "../experiments/gateway_replanning/mod.rs"]
mod gateway_replanning;
#[path = "../experiments/planner_prompt_shaping/mod.rs"]
mod planner_prompt_shaping;
#[path = "../experiments/planner_selection/mod.rs"]
mod planner_selection;
#[path = "../experiments/promotion_environment_provenance/mod.rs"]
mod promotion_environment_provenance;
#[path = "../experiments/promotion_provenance/mod.rs"]
mod promotion_provenance;
#[path = "../experiments/promotion_rules/mod.rs"]
mod promotion_rules;
#[path = "../experiments/provenance_backfill/mod.rs"]
mod provenance_backfill;
#[path = "../experiments/provenance_lag_budgets/mod.rs"]
mod provenance_lag_budgets;
#[path = "../experiments/recovery_skill_patterns/mod.rs"]
mod recovery_skill_patterns;
#[path = "../experiments/resume_policy/mod.rs"]
mod resume_policy;
#[path = "../experiments/rollback_rules/mod.rs"]
mod rollback_rules;
#[path = "../experiments/ros2_observation_ingestion/mod.rs"]
mod ros2_observation_ingestion;
#[path = "../experiments/surface_lag_budget_calibration/mod.rs"]
mod surface_lag_budget_calibration;
#[path = "../experiments/tool_output_validation/mod.rs"]
mod tool_output_validation;

fn main() -> Result<()> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let write_docs = input_write_docs();
    let provider_validation_report = cross_provider_validation::run_suite(&root)?;
    let contradiction_report = cross_suite_contradiction::run_suite(&root)?;
    let carryover_report = cross_train_lag_carryover::run_suite(&root)?;
    let frontier_replay_report = frontier_snapshot_replay::run_suite(&root)?;
    let planner_report = planner_selection::run_suite(&root)?;
    let artifact_trust_report = artifact_trust_decay::run_suite(&root)?;
    let mirroring_drift_report = artifact_mirroring_drift::run_suite(&root)?;
    let provenance_lag_report = provenance_lag_budgets::run_suite(&root)?;
    let cross_repo_stitch_report = cross_repo_provenance_stitching::run_suite(&root)?;
    let provenance_backfill_report = provenance_backfill::run_suite(&root)?;
    let environment_provenance_report = promotion_environment_provenance::run_suite(&root)?;
    let provenance_report = promotion_provenance::run_suite(&root)?;
    let promotion_report = promotion_rules::run_suite(&root)?;
    let prompt_shape_report = planner_prompt_shaping::run_suite(&root)?;
    let recovery_pattern_report = recovery_skill_patterns::run_suite(&root)?;
    let rollback_report = rollback_rules::run_suite(&root)?;
    let ingestion_report = ros2_observation_ingestion::run_suite(&root)?;
    let surface_lag_report = surface_lag_budget_calibration::run_suite(&root)?;
    let resume_report = resume_policy::run_suite(&root)?;
    let gateway_report = gateway_replanning::run_suite(&root)?;
    let fusion_report = backend_observation_fusion::run_suite(&root)?;
    let validation_report = tool_output_validation::run_suite(&root)?;

    println!("suite=cross_provider_validation");
    println!(
        "{}",
        cross_provider_validation::render_summary(&provider_validation_report)
    );
    println!("suite=cross_suite_contradiction");
    println!(
        "{}",
        cross_suite_contradiction::render_summary(&contradiction_report)
    );
    println!("suite=cross_train_lag_carryover");
    println!(
        "{}",
        cross_train_lag_carryover::render_summary(&carryover_report)
    );
    println!("suite=frontier_snapshot_replay");
    println!(
        "{}",
        frontier_snapshot_replay::render_summary(&frontier_replay_report)
    );
    println!("suite=planner_selection");
    println!("{}", planner_selection::render_summary(&planner_report));
    println!("suite=artifact_trust_decay");
    println!(
        "{}",
        artifact_trust_decay::render_summary(&artifact_trust_report)
    );
    println!("suite=artifact_mirroring_drift");
    println!(
        "{}",
        artifact_mirroring_drift::render_summary(&mirroring_drift_report)
    );
    println!("suite=provenance_lag_budgets");
    println!(
        "{}",
        provenance_lag_budgets::render_summary(&provenance_lag_report)
    );
    println!("suite=cross_repo_provenance_stitching");
    println!(
        "{}",
        cross_repo_provenance_stitching::render_summary(&cross_repo_stitch_report)
    );
    println!("suite=provenance_backfill");
    println!(
        "{}",
        provenance_backfill::render_summary(&provenance_backfill_report)
    );
    println!("suite=promotion_environment_provenance");
    println!(
        "{}",
        promotion_environment_provenance::render_summary(&environment_provenance_report)
    );
    println!("suite=promotion_provenance");
    println!(
        "{}",
        promotion_provenance::render_summary(&provenance_report)
    );
    println!("suite=promotion_rules");
    println!("{}", promotion_rules::render_summary(&promotion_report));
    println!("suite=planner_prompt_shaping");
    println!(
        "{}",
        planner_prompt_shaping::render_summary(&prompt_shape_report)
    );
    println!("suite=recovery_skill_patterns");
    println!(
        "{}",
        recovery_skill_patterns::render_summary(&recovery_pattern_report)
    );
    println!("suite=rollback_rules");
    println!("{}", rollback_rules::render_summary(&rollback_report));
    println!("suite=ros2_observation_ingestion");
    println!(
        "{}",
        ros2_observation_ingestion::render_summary(&ingestion_report)
    );
    println!("suite=surface_lag_budget_calibration");
    println!(
        "{}",
        surface_lag_budget_calibration::render_summary(&surface_lag_report)
    );
    println!("suite=resume_policy");
    println!("{}", resume_policy::render_summary(&resume_report));
    println!("suite=gateway_replanning");
    println!("{}", gateway_replanning::render_summary(&gateway_report));
    println!("suite=backend_observation_fusion");
    println!(
        "{}",
        backend_observation_fusion::render_summary(&fusion_report)
    );
    println!("suite=tool_output_validation");
    println!(
        "{}",
        tool_output_validation::render_summary(&validation_report)
    );

    if write_docs {
        write_docs_bundle(
            &root,
            &provider_validation_report,
            &contradiction_report,
            &carryover_report,
            &frontier_replay_report,
            &planner_report,
            &artifact_trust_report,
            &mirroring_drift_report,
            &provenance_lag_report,
            &cross_repo_stitch_report,
            &provenance_backfill_report,
            &environment_provenance_report,
            &provenance_report,
            &promotion_report,
            &prompt_shape_report,
            &recovery_pattern_report,
            &rollback_report,
            &ingestion_report,
            &surface_lag_report,
            &resume_report,
            &gateway_report,
            &fusion_report,
            &validation_report,
        )?;
        println!("docs_written=docs/experiments.md,docs/decisions.md,docs/interfaces.md");
    }

    Ok(())
}

fn input_write_docs() -> bool {
    env::args().skip(1).any(|arg| arg == "--write-docs")
        || env::var("ROBOCLAW_WRITE_EXPERIMENT_DOCS")
            .map(|value| matches!(value.to_lowercase().as_str(), "1" | "true" | "yes" | "on"))
            .unwrap_or(false)
}

fn write_docs_bundle(
    root: &PathBuf,
    provider_validation_report: &cross_provider_validation::ProviderValidationExperimentReport,
    contradiction_report: &cross_suite_contradiction::ContradictionExperimentReport,
    carryover_report: &cross_train_lag_carryover::CrossTrainLagExperimentReport,
    frontier_replay_report: &frontier_snapshot_replay::FrontierReplayExperimentReport,
    planner_report: &planner_selection::ExperimentReport,
    artifact_trust_report: &artifact_trust_decay::ArtifactTrustExperimentReport,
    mirroring_drift_report: &artifact_mirroring_drift::MirroringDriftExperimentReport,
    provenance_lag_report: &provenance_lag_budgets::ProvenanceLagExperimentReport,
    cross_repo_stitch_report: &cross_repo_provenance_stitching::CrossRepoStitchExperimentReport,
    provenance_backfill_report: &provenance_backfill::ProvenanceBackfillExperimentReport,
    environment_provenance_report:
        &promotion_environment_provenance::PromotionEnvironmentExperimentReport,
    provenance_report: &promotion_provenance::PromotionProvenanceExperimentReport,
    promotion_report: &promotion_rules::PromotionExperimentReport,
    prompt_shape_report: &planner_prompt_shaping::PromptShapeExperimentReport,
    recovery_pattern_report: &recovery_skill_patterns::RecoveryPatternExperimentReport,
    rollback_report: &rollback_rules::RollbackExperimentReport,
    ingestion_report: &ros2_observation_ingestion::IngestionExperimentReport,
    surface_lag_report: &surface_lag_budget_calibration::SurfaceLagCalibrationExperimentReport,
    resume_report: &resume_policy::ResumeExperimentReport,
    gateway_report: &gateway_replanning::GatewayExperimentReport,
    fusion_report: &backend_observation_fusion::FusionExperimentReport,
    validation_report: &tool_output_validation::ValidationExperimentReport,
) -> Result<()> {
    fs::write(
        root.join("docs/experiments.md"),
        render_experiments_doc(
            provider_validation_report,
            contradiction_report,
            carryover_report,
            frontier_replay_report,
            planner_report,
            artifact_trust_report,
            mirroring_drift_report,
            provenance_lag_report,
            cross_repo_stitch_report,
            provenance_backfill_report,
            environment_provenance_report,
            provenance_report,
            promotion_report,
            prompt_shape_report,
            recovery_pattern_report,
            rollback_report,
            ingestion_report,
            surface_lag_report,
            resume_report,
            gateway_report,
            fusion_report,
            validation_report,
        ),
    )?;
    fs::write(
        root.join("docs/decisions.md"),
        render_decisions_doc(
            provider_validation_report,
            contradiction_report,
            carryover_report,
            frontier_replay_report,
            planner_report,
            artifact_trust_report,
            mirroring_drift_report,
            provenance_lag_report,
            cross_repo_stitch_report,
            provenance_backfill_report,
            environment_provenance_report,
            provenance_report,
            promotion_report,
            prompt_shape_report,
            recovery_pattern_report,
            rollback_report,
            ingestion_report,
            surface_lag_report,
            resume_report,
            gateway_report,
            fusion_report,
            validation_report,
        ),
    )?;
    fs::write(
        root.join("docs/interfaces.md"),
        render_interfaces_doc(
            provider_validation_report,
            contradiction_report,
            carryover_report,
            frontier_replay_report,
            planner_report,
            artifact_trust_report,
            mirroring_drift_report,
            provenance_lag_report,
            cross_repo_stitch_report,
            provenance_backfill_report,
            environment_provenance_report,
            provenance_report,
            promotion_report,
            prompt_shape_report,
            recovery_pattern_report,
            rollback_report,
            ingestion_report,
            surface_lag_report,
            resume_report,
            gateway_report,
            fusion_report,
            validation_report,
        ),
    )?;
    Ok(())
}

fn render_experiments_doc(
    provider_validation_report: &cross_provider_validation::ProviderValidationExperimentReport,
    contradiction_report: &cross_suite_contradiction::ContradictionExperimentReport,
    carryover_report: &cross_train_lag_carryover::CrossTrainLagExperimentReport,
    frontier_replay_report: &frontier_snapshot_replay::FrontierReplayExperimentReport,
    planner_report: &planner_selection::ExperimentReport,
    artifact_trust_report: &artifact_trust_decay::ArtifactTrustExperimentReport,
    mirroring_drift_report: &artifact_mirroring_drift::MirroringDriftExperimentReport,
    provenance_lag_report: &provenance_lag_budgets::ProvenanceLagExperimentReport,
    cross_repo_stitch_report: &cross_repo_provenance_stitching::CrossRepoStitchExperimentReport,
    provenance_backfill_report: &provenance_backfill::ProvenanceBackfillExperimentReport,
    environment_provenance_report:
        &promotion_environment_provenance::PromotionEnvironmentExperimentReport,
    provenance_report: &promotion_provenance::PromotionProvenanceExperimentReport,
    promotion_report: &promotion_rules::PromotionExperimentReport,
    prompt_shape_report: &planner_prompt_shaping::PromptShapeExperimentReport,
    recovery_pattern_report: &recovery_skill_patterns::RecoveryPatternExperimentReport,
    rollback_report: &rollback_rules::RollbackExperimentReport,
    ingestion_report: &ros2_observation_ingestion::IngestionExperimentReport,
    surface_lag_report: &surface_lag_budget_calibration::SurfaceLagCalibrationExperimentReport,
    resume_report: &resume_policy::ResumeExperimentReport,
    gateway_report: &gateway_replanning::GatewayExperimentReport,
    fusion_report: &backend_observation_fusion::FusionExperimentReport,
    validation_report: &tool_output_validation::ValidationExperimentReport,
) -> String {
    let mut markdown = String::new();
    markdown.push_str("# Experiments\n\n");
    markdown
        .push_str("Generated by `cargo run --example planner_experiments -- --write-docs`.\n\n");
    markdown.push_str("This repository treats design work as a sequence of comparable experiments. Stable runtime code stays in `crates/`; design candidates stay in `experiments/` until repeated case sets justify promotion.\n\n");
    markdown.push_str("## Active Suites\n\n");
    markdown.push_str(
        "- `cross_provider_validation`: validate the current planner frontier across provider adapters.\n",
    );
    markdown.push_str(
        "- `cross_suite_contradiction`: detect when local suite frontiers and promotion policy disagree.\n",
    );
    markdown.push_str(
        "- `cross_train_lag_carryover`: decide whether unresolved publication lag should continue, resolve, or block across release cuts.\n",
    );
    markdown.push_str(
        "- `frontier_snapshot_replay`: replay versioned provider/model evidence before promoting a provisional frontier.\n",
    );
    markdown.push_str("- `planner_selection`: choose which YAML skill should run.\n");
    markdown.push_str(
        "- `artifact_trust_decay`: decide how artifact trust should fall off when changelog, release notes, and rollback notes disagree across release trains.\n",
    );
    markdown.push_str(
        "- `artifact_mirroring_drift`: decide how publication mirrors should drift when package registries and docs portals fall out of sync.\n",
    );
    markdown.push_str(
        "- `provenance_lag_budgets`: decide when staggered publication delays should stay pending, become superseded, or block provenance updates.\n",
    );
    markdown.push_str(
        "- `cross_repo_provenance_stitching`: decide how provenance should be stitched when release evidence is split across multiple repositories.\n",
    );
    markdown.push_str(
        "- `provenance_backfill`: decide whether missing release provenance can be safely reconstructed from changelog and release-note artifacts.\n",
    );
    markdown.push_str(
        "- `promotion_environment_provenance`: decide whether a promoted runtime surface still has coherent provenance across deployment environments.\n",
    );
    markdown.push_str(
        "- `promotion_provenance`: decide whether a promoted runtime surface still has a continuous documented release lineage.\n",
    );
    markdown.push_str(
        "- `promotion_rules`: decide when an experimental frontier is mature enough to move into the stable runtime.\n",
    );
    markdown.push_str(
        "- `planner_prompt_shaping`: choose how planner prompt text and schema constraints should be presented.\n",
    );
    markdown.push_str(
        "- `recovery_skill_patterns`: choose what kind of recovery skill sequence should run after a failure.\n",
    );
    markdown.push_str(
        "- `rollback_rules`: decide when a previously promoted runtime surface should stay, roll back, or be replaced.\n",
    );
    markdown.push_str(
        "- `ros2_observation_ingestion`: choose how ROS2 topic streams should reduce into replanning context.\n",
    );
    markdown.push_str(
        "- `surface_lag_budget_calibration`: choose how each publication surface should calibrate its lag budget from observed traces.\n",
    );
    markdown.push_str("- `resume_policy`: choose which step to resume from after recovery.\n");
    markdown.push_str(
        "- `gateway_replanning`: choose what the gateway loop does after each execution outcome.\n",
    );
    markdown.push_str(
        "- `backend_observation_fusion`: choose how backend state and sensor observations are fused before replanning.\n",
    );
    markdown.push_str(
        "- `tool_output_validation`: choose how `expect` contracts should match tool outputs before retry or replan.\n\n",
    );
    markdown.push_str(&cross_provider_validation::render_experiments_section(
        provider_validation_report,
    ));
    markdown.push('\n');
    markdown.push_str(&cross_suite_contradiction::render_experiments_section(
        contradiction_report,
    ));
    markdown.push('\n');
    markdown.push_str(&cross_train_lag_carryover::render_experiments_section(
        carryover_report,
    ));
    markdown.push('\n');
    markdown.push_str(&frontier_snapshot_replay::render_experiments_section(
        frontier_replay_report,
    ));
    markdown.push('\n');
    markdown.push_str(&planner_selection::render_experiments_section(
        planner_report,
    ));
    markdown.push('\n');
    markdown.push_str(&artifact_trust_decay::render_experiments_section(
        artifact_trust_report,
    ));
    markdown.push('\n');
    markdown.push_str(&artifact_mirroring_drift::render_experiments_section(
        mirroring_drift_report,
    ));
    markdown.push('\n');
    markdown.push_str(&provenance_lag_budgets::render_experiments_section(
        provenance_lag_report,
    ));
    markdown.push('\n');
    markdown.push_str(
        &cross_repo_provenance_stitching::render_experiments_section(cross_repo_stitch_report),
    );
    markdown.push('\n');
    markdown.push_str(&provenance_backfill::render_experiments_section(
        provenance_backfill_report,
    ));
    markdown.push('\n');
    markdown.push_str(
        &promotion_environment_provenance::render_experiments_section(
            environment_provenance_report,
        ),
    );
    markdown.push('\n');
    markdown.push_str(&promotion_provenance::render_experiments_section(
        provenance_report,
    ));
    markdown.push('\n');
    markdown.push_str(&promotion_rules::render_experiments_section(
        promotion_report,
    ));
    markdown.push('\n');
    markdown.push_str(&planner_prompt_shaping::render_experiments_section(
        prompt_shape_report,
    ));
    markdown.push('\n');
    markdown.push_str(&recovery_skill_patterns::render_experiments_section(
        recovery_pattern_report,
    ));
    markdown.push('\n');
    markdown.push_str(&rollback_rules::render_experiments_section(rollback_report));
    markdown.push('\n');
    markdown.push_str(&ros2_observation_ingestion::render_experiments_section(
        ingestion_report,
    ));
    markdown.push('\n');
    markdown.push_str(&surface_lag_budget_calibration::render_experiments_section(
        surface_lag_report,
    ));
    markdown.push('\n');
    markdown.push_str(&resume_policy::render_experiments_section(resume_report));
    markdown.push('\n');
    markdown.push_str(&gateway_replanning::render_experiments_section(
        gateway_report,
    ));
    markdown.push('\n');
    markdown.push_str(&backend_observation_fusion::render_experiments_section(
        fusion_report,
    ));
    markdown.push('\n');
    markdown.push_str(&tool_output_validation::render_experiments_section(
        validation_report,
    ));
    markdown
}

fn render_decisions_doc(
    provider_validation_report: &cross_provider_validation::ProviderValidationExperimentReport,
    contradiction_report: &cross_suite_contradiction::ContradictionExperimentReport,
    carryover_report: &cross_train_lag_carryover::CrossTrainLagExperimentReport,
    frontier_replay_report: &frontier_snapshot_replay::FrontierReplayExperimentReport,
    planner_report: &planner_selection::ExperimentReport,
    artifact_trust_report: &artifact_trust_decay::ArtifactTrustExperimentReport,
    mirroring_drift_report: &artifact_mirroring_drift::MirroringDriftExperimentReport,
    provenance_lag_report: &provenance_lag_budgets::ProvenanceLagExperimentReport,
    cross_repo_stitch_report: &cross_repo_provenance_stitching::CrossRepoStitchExperimentReport,
    provenance_backfill_report: &provenance_backfill::ProvenanceBackfillExperimentReport,
    environment_provenance_report:
        &promotion_environment_provenance::PromotionEnvironmentExperimentReport,
    provenance_report: &promotion_provenance::PromotionProvenanceExperimentReport,
    promotion_report: &promotion_rules::PromotionExperimentReport,
    prompt_shape_report: &planner_prompt_shaping::PromptShapeExperimentReport,
    recovery_pattern_report: &recovery_skill_patterns::RecoveryPatternExperimentReport,
    rollback_report: &rollback_rules::RollbackExperimentReport,
    ingestion_report: &ros2_observation_ingestion::IngestionExperimentReport,
    surface_lag_report: &surface_lag_budget_calibration::SurfaceLagCalibrationExperimentReport,
    resume_report: &resume_policy::ResumeExperimentReport,
    gateway_report: &gateway_replanning::GatewayExperimentReport,
    fusion_report: &backend_observation_fusion::FusionExperimentReport,
    validation_report: &tool_output_validation::ValidationExperimentReport,
) -> String {
    let mut markdown = String::new();
    markdown.push_str("# Decisions\n\n");
    markdown
        .push_str("Generated by `cargo run --example planner_experiments -- --write-docs`.\n\n");
    markdown.push_str("## Accepted Now\n\n");
    markdown.push_str("- Keep stable runtime abstractions narrow and product-facing.\n");
    markdown.push_str("- Put alternative designs into experiment suites with shared cases and generated comparison docs.\n");
    markdown.push_str("- Use provisional references only to guide the next experiment cycle; do not treat them as final architecture.\n\n");
    markdown.push_str("## Narrow Stable Core Now\n\n");
    markdown.push_str("- Keep `roboclaw-agent::Planner -> PlanDecision` as the only stable planning contract. Skill-ranking heuristics and provider adapters remain experimental, but catalog-constrained planning is now the narrow boundary that survives the current suite set.\n");
    markdown.push_str("- Keep `roboclaw-skills::SkillCatalog` plus YAML execution metadata stable. The current runtime can stay narrow around `expect`, `max_retries`, `resume_from_step`, `recovery_for`, `resume_original_instruction`, and `supports_checkpoint_resume`.\n");
    markdown.push_str("- Keep the gateway loop stable only at the orchestration level: `instruction -> plan -> execute -> observe -> memory -> resume/replan`. The winning recovery surfaces reinforce metadata-driven control rather than new type layers.\n");
    markdown.push_str("- Keep `roboclaw-tools::Tool`, `roboclaw-sim::RobotBackend`, `roboclaw-ros2::Ros2Bridge`, and `roboclaw-memory::Memory` as product-facing boundaries. Exact reduction, fusion, and matcher policies stay replaceable.\n");
    markdown.push_str("- Keep provenance, promotion, rollback, mirroring, and lag-calibration suites outside the runtime core. They are converging on budget-style governance, but that evidence should constrain policy, not widen the robot runtime API.\n\n");
    markdown.push_str(&cross_provider_validation::render_decisions_section(
        provider_validation_report,
    ));
    markdown.push('\n');
    markdown.push_str(&cross_suite_contradiction::render_decisions_section(
        contradiction_report,
    ));
    markdown.push('\n');
    markdown.push_str(&cross_train_lag_carryover::render_decisions_section(
        carryover_report,
    ));
    markdown.push('\n');
    markdown.push_str(&frontier_snapshot_replay::render_decisions_section(
        frontier_replay_report,
    ));
    markdown.push('\n');
    markdown.push_str(&planner_selection::render_decisions_section(planner_report));
    markdown.push('\n');
    markdown.push_str(&artifact_trust_decay::render_decisions_section(
        artifact_trust_report,
    ));
    markdown.push('\n');
    markdown.push_str(&artifact_mirroring_drift::render_decisions_section(
        mirroring_drift_report,
    ));
    markdown.push('\n');
    markdown.push_str(&provenance_lag_budgets::render_decisions_section(
        provenance_lag_report,
    ));
    markdown.push('\n');
    markdown.push_str(&cross_repo_provenance_stitching::render_decisions_section(
        cross_repo_stitch_report,
    ));
    markdown.push('\n');
    markdown.push_str(&provenance_backfill::render_decisions_section(
        provenance_backfill_report,
    ));
    markdown.push('\n');
    markdown.push_str(&promotion_environment_provenance::render_decisions_section(
        environment_provenance_report,
    ));
    markdown.push('\n');
    markdown.push_str(&promotion_provenance::render_decisions_section(
        provenance_report,
    ));
    markdown.push('\n');
    markdown.push_str(&promotion_rules::render_decisions_section(promotion_report));
    markdown.push('\n');
    markdown.push_str(&planner_prompt_shaping::render_decisions_section(
        prompt_shape_report,
    ));
    markdown.push('\n');
    markdown.push_str(&recovery_skill_patterns::render_decisions_section(
        recovery_pattern_report,
    ));
    markdown.push('\n');
    markdown.push_str(&rollback_rules::render_decisions_section(rollback_report));
    markdown.push('\n');
    markdown.push_str(&ros2_observation_ingestion::render_decisions_section(
        ingestion_report,
    ));
    markdown.push('\n');
    markdown.push_str(&surface_lag_budget_calibration::render_decisions_section(
        surface_lag_report,
    ));
    markdown.push('\n');
    markdown.push_str(&resume_policy::render_decisions_section(resume_report));
    markdown.push('\n');
    markdown.push_str(&gateway_replanning::render_decisions_section(
        gateway_report,
    ));
    markdown.push('\n');
    markdown.push_str(&backend_observation_fusion::render_decisions_section(
        fusion_report,
    ));
    markdown.push('\n');
    markdown.push_str(&tool_output_validation::render_decisions_section(
        validation_report,
    ));
    markdown.push_str("\n## Next Suites\n\n");
    markdown.push_str("- Topic QoS and delayed-telemetry tolerance across ROS2 transports.\n");
    markdown.push_str("- Drift-aware case generation when a promoted reference starts regressing across snapshots.\n");
    markdown.push_str("- Promotion handoff rules for moving a provisional frontier into the stable core without copying its full experimental machinery.\n");
    markdown
}

fn render_interfaces_doc(
    _provider_validation_report: &cross_provider_validation::ProviderValidationExperimentReport,
    _contradiction_report: &cross_suite_contradiction::ContradictionExperimentReport,
    _carryover_report: &cross_train_lag_carryover::CrossTrainLagExperimentReport,
    _frontier_replay_report: &frontier_snapshot_replay::FrontierReplayExperimentReport,
    planner_report: &planner_selection::ExperimentReport,
    _artifact_trust_report: &artifact_trust_decay::ArtifactTrustExperimentReport,
    _mirroring_drift_report: &artifact_mirroring_drift::MirroringDriftExperimentReport,
    _provenance_lag_report: &provenance_lag_budgets::ProvenanceLagExperimentReport,
    _cross_repo_stitch_report: &cross_repo_provenance_stitching::CrossRepoStitchExperimentReport,
    _provenance_backfill_report: &provenance_backfill::ProvenanceBackfillExperimentReport,
    _environment_provenance_report:
        &promotion_environment_provenance::PromotionEnvironmentExperimentReport,
    _provenance_report: &promotion_provenance::PromotionProvenanceExperimentReport,
    _promotion_report: &promotion_rules::PromotionExperimentReport,
    _prompt_shape_report: &planner_prompt_shaping::PromptShapeExperimentReport,
    _recovery_pattern_report: &recovery_skill_patterns::RecoveryPatternExperimentReport,
    _rollback_report: &rollback_rules::RollbackExperimentReport,
    _ingestion_report: &ros2_observation_ingestion::IngestionExperimentReport,
    _surface_lag_report: &surface_lag_budget_calibration::SurfaceLagCalibrationExperimentReport,
    _resume_report: &resume_policy::ResumeExperimentReport,
    _gateway_report: &gateway_replanning::GatewayExperimentReport,
    _fusion_report: &backend_observation_fusion::FusionExperimentReport,
    _validation_report: &tool_output_validation::ValidationExperimentReport,
) -> String {
    let mut markdown = String::new();
    markdown.push_str("# Interfaces\n\n");
    markdown
        .push_str("Generated by `cargo run --example planner_experiments -- --write-docs`.\n\n");
    markdown.push_str("## Stable Core\n\n");
    markdown.push_str("- `roboclaw_agent::Planner`\n");
    markdown.push_str("- `roboclaw_agent::PlanDecision`\n");
    markdown.push_str("- `roboclaw_skills::SkillCatalog`\n");
    markdown.push_str("- `roboclaw_tools::Tool`\n");
    markdown.push_str("- `roboclaw_sim::RobotBackend`\n");
    markdown.push_str("- `roboclaw_ros2::Ros2Bridge`\n");
    markdown.push_str("- `roboclaw_memory::Memory`\n");
    markdown.push_str("- Skill YAML files under `skills/`\n");
    markdown.push_str("- Gateway/runtime behavior used by examples and ROS2 flow\n\n");
    markdown.push_str("## Stable Core Shape\n\n");
    markdown.push_str("- Planning stays stable only at the boundary: choose one skill from the catalog and return a `PlanDecision`.\n");
    markdown.push_str("- Execution stays stable only at the orchestration boundary: execute tool-backed YAML steps, persist memory, and either continue, resume from checkpoint metadata, or request replanning.\n");
    markdown.push_str("- Recovery stays stable as YAML metadata, not as new runtime abstractions. Core fields are `expect`, `max_retries`, `resume_from_step`, `recovery_for`, `resume_original_instruction`, and `supports_checkpoint_resume`.\n");
    markdown.push_str("- Observation handling stays stable as an input boundary, not as one winning reducer. Core accepts reduced context from ROS2/backend layers while reducer and fusion policy stay experimental.\n");
    markdown.push_str("- Provenance and release-governance policies remain outside core even when budget-style variants win repeatedly.\n\n");
    markdown.push_str("## Experimental Boundaries\n\n");
    markdown.push_str(&cross_provider_validation::render_interfaces_section());
    markdown.push('\n');
    markdown.push_str(&cross_suite_contradiction::render_interfaces_section());
    markdown.push('\n');
    markdown.push_str(&cross_train_lag_carryover::render_interfaces_section());
    markdown.push('\n');
    markdown.push_str(&frontier_snapshot_replay::render_interfaces_section());
    markdown.push('\n');
    markdown.push_str(&planner_selection::render_interfaces_section());
    markdown.push('\n');
    markdown.push_str(&artifact_trust_decay::render_interfaces_section());
    markdown.push('\n');
    markdown.push_str(&artifact_mirroring_drift::render_interfaces_section());
    markdown.push('\n');
    markdown.push_str(&provenance_lag_budgets::render_interfaces_section());
    markdown.push('\n');
    markdown.push_str(&cross_repo_provenance_stitching::render_interfaces_section());
    markdown.push('\n');
    markdown.push_str(&provenance_backfill::render_interfaces_section());
    markdown.push('\n');
    markdown.push_str(&promotion_environment_provenance::render_interfaces_section());
    markdown.push('\n');
    markdown.push_str(&promotion_provenance::render_interfaces_section());
    markdown.push('\n');
    markdown.push_str(&promotion_rules::render_interfaces_section());
    markdown.push('\n');
    markdown.push_str(&planner_prompt_shaping::render_interfaces_section());
    markdown.push('\n');
    markdown.push_str(&recovery_skill_patterns::render_interfaces_section());
    markdown.push('\n');
    markdown.push_str(&rollback_rules::render_interfaces_section());
    markdown.push('\n');
    markdown.push_str(&ros2_observation_ingestion::render_interfaces_section());
    markdown.push('\n');
    markdown.push_str(&surface_lag_budget_calibration::render_interfaces_section());
    markdown.push('\n');
    markdown.push_str(&resume_policy::render_interfaces_section());
    markdown.push('\n');
    markdown.push_str(&gateway_replanning::render_interfaces_section());
    markdown.push('\n');
    markdown.push_str(&backend_observation_fusion::render_interfaces_section());
    markdown.push('\n');
    markdown.push_str(&tool_output_validation::render_interfaces_section());
    markdown.push_str("\n## Region Split\n\n");
    markdown.push_str("- Stable region: `crates/`, `skills/`, `examples/pick_and_place.rs`\n");
    markdown.push_str("- Experimental region: `experiments/`, `docs/experiments.md`, `docs/decisions.md`, `docs/interfaces.md`\n");
    markdown.push_str("- Promotion rule: only abstractions that survive multiple experiment suites and case expansions can move into the stable region.\n\n");
    markdown.push_str("## Generated From\n\n");
    markdown.push_str(&format!(
        "- planner suite command tag: `{}`\n",
        planner_report.generated_by
    ));
    markdown
}
