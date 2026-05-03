# Claude Code provider switching
# Source this file from ~/.bashrc
#
# This is a thin wrapper around the Rust ccswitch binary.
# The binary handles: SQLite DB, multi-key rotation, settings.json atomic writes.

unset ANTHROPIC_API_KEY

# ── API keys (exported so Rust binary can import on first run) ───────────────
export GLM_API_KEY="2759e573bb3c40b9a88053f751f31638.WF3F2ZEVj9s158IK"
export MINIMAX_API_KEY="sk-cp-4lgbzZU_582H4nizTfJRP8KrLPmSH-9rMKpvtwk4D9HQrz6HEQCCRbI82-Qe-_pJfMajKdKTFpHosedLeBLwpdF9FcLzYHc66udQyWPpxUNkZHcnB2fsVCY"
export ZHONGZHUAN_API_KEY="sk-zNQxU498oqb99SikkJmvvjhib24BdypczoMETmPnDwYANzLm"
export KIMI_API_KEY="sk-kimi-T7bFyp96UIJLOr5YJUFkoI3786ozEyUIQ5MKSdZHUcjTaks4LnCxKgGpvtNxg5jD"

# ── Binary path ──────────────────────────────────────────────────────────────
_CCSWITCH_BIN="${CCSWITCH_BIN:-$HOME/workspace/ccswitch/target/release/ccswitch}"

# ── Helper: run binary and eval export/unset for current shell ───────────────
_ccswitch_run() {
    local output
    output=$("$_CCSWITCH_BIN" "$@" 2>/dev/null)
    local status=$?
    # Eval export/unset commands so current shell stays in sync
    while IFS= read -r line; do
        if [[ "$line" == export\ * || "$line" == unset\ * ]]; then
            eval "$line"
        fi
    done <<< "$output"
    # Print non-shell-command lines
    while IFS= read -r line; do
        if [[ "$line" != export\ * && "$line" != unset\ * ]]; then
            echo "$line"
        fi
    done <<< "$output"
    return $status
}

# ── ccswitch CLI (delegates to Rust binary) ──────────────────────────────────
ccswitch() {
    if [[ ! -x "$_CCSWITCH_BIN" ]]; then
        echo "Error: ccswitch binary not found at $_CCSWITCH_BIN" >&2
        echo "Run:  cd ~/workspace/ccswitch && cargo build --release" >&2
        return 1
    fi

    case "$1" in
        "")
            _ccswitch_run --help
            ;;
        tui)
            # TUI needs direct terminal access; bypass output capture
            "$_CCSWITCH_BIN" "$@"
            ;;
        list|status|use|key|health|usage)
            _ccswitch_run "$@"
            ;;
        *)
            _ccswitch_run "$@"
            ;;
    esac
}

# ── Backward-compatible aliases ───────────────────────────────────────────────
kimi()  { _ccswitch_run use kimi; }
glm()   { _ccswitch_run use glm; }
zz()    { _ccswitch_run use zz; }
minimax() { _ccswitch_run use minimax; }
cc()    { _ccswitch_run use cc; }
model() { _ccswitch_run status; }
alias ccsp='gtz && claude --dangerously-skip-permissions'
