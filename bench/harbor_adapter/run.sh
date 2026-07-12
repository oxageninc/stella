#!/usr/bin/env bash
#
# Run SWE-bench (Harbor) against the Stella coding CLI.
#
# Usage:
#   STELLA_MODEL=zai/glm-5.2 ./run.sh
#   TASK_IDS="django__django-11099" N_CONCURRENT=1 ./run.sh
#   STELLA_BUDGET=10.0 ./run.sh
#
# Prereqs: docker running; provider API key exported (ZAI_API_KEY, etc.)
# Requires: stella_harbor package installed (see below)

set -euo pipefail

cd "$(dirname "$0")"
REPO_ROOT="$(cd ../.. && pwd)"
ADAPTER_DIR="$(pwd)"

# Ensure Stella is built
if [ ! -f "$REPO_ROOT/target/release/stella" ]; then
    echo "Building Stella..."
    cd "$REPO_ROOT"
    cargo build --release -p stella-cli
    cd "$ADAPTER_DIR"
fi

# Configuration
AGENT="${AGENT:-stella}"
MODEL_SLUG="${STELLA_MODEL:-zai/glm-5.2}"
DATASET="${DATASET:-swe-bench/swe-bench-verified}"
N_CONCURRENT="${N_CONCURRENT:-4}"
N_ATTEMPTS="${N_ATTEMPTS:-1}"
JOBS_DIR="${JOBS_DIR:-./results-stella}"

# Export for the adapter to pick up
export STELLA_MODEL="$MODEL_SLUG"
export STELLA_BUDGET="${STELLA_BUDGET:-5.0}"
export STELLA_BINARY="$REPO_ROOT/target/release/stella"
export STELLA_BASE_URL="${STELLA_BASE_URL:-https://api.z.ai/api/coding/paas/v4}"

# Ensure adapter is installed and importable
echo "Setting up Stella Harbor adapter..."
python3 -m pip install -e "$ADAPTER_DIR" --user --break-system-packages --quiet

# Get user site-packages for PYTHONPATH
USER_SITE=$(python3 -c "import site; print(site.USER_SITE)")
echo "User site-packages: $USER_SITE"

# Verify import
python3 -c "from stella_harbor import StellaAgent; print('✓ Stella agent importable')" 2>/dev/null || {
    echo "Error: stella_harbor package not importable"
    exit 1
}

# Build task ID args
TASK_ID_ARGS=()
if [ -n "${TASK_IDS:-}" ]; then
    for t in $TASK_IDS; do
        TASK_ID_ARGS+=(--include-task-name "*$t")
    done
fi

# Locate Harbor SWE-bench runner (oxagen-platform)
OXAGEN_PLATFORM="${OXAGEN_PLATFORM:-$HOME/Workspaces/oxagen-platform}"
HARBOR_RUNNER="$OXAGEN_PLATFORM/bench/swe-bench/run.sh"

if [ ! -f "$HARBOR_RUNNER" ]; then
    echo "Error: Cannot find Harbor SWE-bench runner at $HARBOR_RUNNER"
    echo "Set OXAGEN_PLATFORM to the oxagen-platform repo path."
    exit 1
fi

echo "=== Stella SWE-bench run ==="
echo "Agent: $AGENT"
echo "Model: $MODEL_SLUG"
echo "Dataset: $DATASET"
echo "Concurrent: $N_CONCURRENT"
echo "Budget: \$${STELLA_BUDGET} per task"
echo "Jobs dir: $JOBS_DIR"
echo ""

# Build Harbor args (pass stella-specific args via env vars)
HARBOR_ARGS=(
    --dataset "$DATASET"
    -m "$MODEL_SLUG"
    --n-concurrent "$N_CONCURRENT"
    --n-attempts "$N_ATTEMPTS"
    --jobs-dir "$JOBS_DIR"
)

if [ ${#TASK_ID_ARGS[@]} -gt 0 ]; then
    HARBOR_ARGS+=("${TASK_ID_ARGS[@]}")
fi

if [ -n "${HARBOR_EXTRA:-}" ]; then
    HARBOR_ARGS+=($HARBOR_EXTRA)
fi

# Run Harbor with stella agent
# Set PYTHONPATH so Harbor's uv run can find stella_harbor
echo "Running Harbor..."
cd "$OXAGEN_PLATFORM/bench/swe-bench"
PYTHONPATH="${PYTHONPATH:-}:${USER_SITE}" AGENT=stella exec "$HARBOR_RUNNER" "${HARBOR_ARGS[@]}"
