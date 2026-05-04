# Claude Code provider switching
# Source this file from ~/.bashrc
#
# This is a thin wrapper around the Rust ccswitch binary.
# The binary handles: SQLite DB, multi-key rotation, settings.json atomic writes.

unset ANTHROPIC_API_KEY

# ── Load API keys from .env (gitignored) ─────────────────────────────────────
_CCSWITCH_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
if [[ -f "$_CCSWITCH_DIR/.env" ]]; then
    source "$_CCSWITCH_DIR/.env"
fi

# ── Binary path ──────────────────────────────────────────────────────────────
_CCSWITCH_BIN="${CCSWITCH_BIN:-$HOME/workspace/ccswitch/target/release/ccswitch}"

# ── Helper: run binary and eval export/unset for current shell ───────────────
_ccswitch_run() {
    local output
    output=$("$_CCSWITCH_BIN" "$@" 2>/dev/null)
    local status=$?
    while IFS= read -r line; do
        if [[ "$line" == export\ * || "$line" == unset\ * ]]; then
            eval "$line"
        else
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
