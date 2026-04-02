#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"

ROS_DISTRO_NAME="${ROS_DISTRO:-jazzy}"
ROS_SETUP="/opt/ros/${ROS_DISTRO_NAME}/setup.bash"
ROS_LIB_DIR="/opt/ros/${ROS_DISTRO_NAME}/lib"

if [[ ! -f "${ROS_SETUP}" ]]; then
  echo "error: ROS 2 setup file not found at ${ROS_SETUP}" >&2
  exit 1
fi

set +u
source "${ROS_SETUP}"
set -u

prepend_path() {
  local prefix="$1"
  local current="${2-}"
  if [[ -n "${current}" ]]; then
    printf '%s:%s' "${prefix}" "${current}"
  else
    printf '%s' "${prefix}"
  fi
}

ensure_test_msgs_linker_inputs() {
  local missing=()
  local lib

  for lib in libtest_msgs__rosidl_generator_c.so libtest_msgs__rosidl_typesupport_c.so; do
    if [[ ! -f "${ROS_LIB_DIR}/${lib}" ]]; then
      missing+=("${lib}")
    fi
  done

  if (( ${#missing[@]} == 0 )); then
    return 0
  fi

  local stub_dir="${ROOT_DIR}/target/ros2-stubs"
  local stub_c="${stub_dir}/empty.c"

  mkdir -p "${stub_dir}"
  printf 'int roboclaw_ros2_stub(void){return 0;}\n' > "${stub_c}"

  cc -shared -fPIC "${stub_c}" -o "${stub_dir}/libtest_msgs__rosidl_generator_c.so"
  cc -shared -fPIC "${stub_c}" -o "${stub_dir}/libtest_msgs__rosidl_typesupport_c.so"

  export LIBRARY_PATH
  export LD_LIBRARY_PATH
  LIBRARY_PATH="$(prepend_path "${stub_dir}" "${LIBRARY_PATH-}")"
  LD_LIBRARY_PATH="$(prepend_path "${stub_dir}" "${LD_LIBRARY_PATH-}")"

  echo "warning: ROS 2 test_msgs libraries are missing in ${ROS_LIB_DIR}" >&2
  echo "warning: using temporary stub libs from ${stub_dir} for local demo linking only" >&2
  echo "warning: install ros-${ROS_DISTRO_NAME}-test-msgs to remove this workaround" >&2
}

ensure_test_msgs_linker_inputs

export ROBOCLAW_ROS2_BRIDGE="${ROBOCLAW_ROS2_BRIDGE:-rclrs}"

cd "${ROOT_DIR}"

if (( $# > 0 )); then
  cargo run --features ros2 --example pick_and_place -- "$@"
else
  cargo run --features ros2 --example pick_and_place
fi
