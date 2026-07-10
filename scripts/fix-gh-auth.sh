#!/usr/bin/env bash
# fix-gh-auth.sh — diagnose and repair GitHub CLI (gh) authentication
#
# Usage:
#   ./scripts/fix-gh-auth.sh           # interactive
#   ./scripts/fix-gh-auth.sh --check   # status only
#   ./scripts/fix-gh-auth.sh --login   # logout bad token + login
#   ./scripts/fix-gh-auth.sh --ssh     # prefer SSH git protocol
#   ./scripts/fix-gh-auth.sh --https   # prefer HTTPS git protocol
#
# Typical fix when you see "The token in default is invalid":
#   1. Logout the stale account
#   2. Login again with a browser or PAT
#   3. Verify api.github.com works

set -euo pipefail

HOST="${GH_HOST:-github.com}"
ACCOUNT="${GH_USER:-maplepreneur}"
MODE="interactive"
GIT_PROTOCOL="" # ssh | https | empty (ask)

RED=$'\033[0;31m'
GREEN=$'\033[0;32m'
YELLOW=$'\033[1;33m'
CYAN=$'\033[0;36m'
BOLD=$'\033[1m'
NC=$'\033[0m'

info()  { printf '%s%s%s\n' "$CYAN" "$*" "$NC"; }
ok()    { printf '%s%s%s\n' "$GREEN" "$*" "$NC"; }
warn()  { printf '%s%s%s\n' "$YELLOW" "$*" "$NC"; }
err()   { printf '%s%s%s\n' "$RED" "$*" "$NC" >&2; }
header(){ printf '\n%s%s%s\n' "$BOLD" "$*" "$NC"; }

usage() {
  sed -n '2,18p' "$0" | sed 's/^# \?//'
  exit 0
}

for arg in "$@"; do
  case "$arg" in
    -h|--help) usage ;;
    --check)   MODE="check" ;;
    --login)   MODE="login" ;;
    --ssh)     GIT_PROTOCOL="ssh" ;;
    --https)   GIT_PROTOCOL="https" ;;
    *)
      err "Unknown option: $arg"
      usage
      ;;
  esac
done

need_cmd() {
  if ! command -v "$1" >/dev/null 2>&1; then
    err "Missing required command: $1"
    case "$1" in
      gh)
        info "Install GitHub CLI:"
        echo "  # Debian/Ubuntu/Zorin"
        echo "  sudo apt update && sudo apt install gh"
        echo "  # or: https://cli.github.com/"
        ;;
    esac
    exit 1
  fi
}

check_status() {
  header "1. gh version"
  gh --version || true

  header "2. auth status ($HOST)"
  # gh auth status exits non-zero when broken; capture both streams
  local status_out
  status_out="$(gh auth status -h "$HOST" 2>&1 || true)"
  printf '%s\n' "$status_out"

  if printf '%s' "$status_out" | grep -qiE 'Failed to log in|token .* invalid|not logged|not logged in'; then
    warn "→ Auth looks BROKEN."
    return 1
  fi
  if printf '%s' "$status_out" | grep -qE 'Logged in to'; then
    ok "→ Auth status reports logged in."
  else
    warn "→ Could not confirm a healthy login."
    return 1
  fi

  header "3. API probe (list 1 repo)"
  if gh api user -q .login 2>/dev/null; then
    ok "→ API token works (whoami above)."
  else
    err "→ API call failed — token is missing, expired, or lacks scopes."
    return 1
  fi

  header "4. GitHub git remote tip"
  if git rev-parse --is-inside-work-tree >/dev/null 2>&1; then
    git remote -v 2>/dev/null || true
  else
    info "(not inside a git repo — skipped remotes)"
  fi

  return 0
}

logout_stale() {
  header "Logout stale credentials"
  # Ignore errors if already logged out
  if gh auth logout -h "$HOST" -u "$ACCOUNT" 2>/dev/null; then
    ok "Logged out: $ACCOUNT @ $HOST"
  else
    # Try without username (active account)
    if gh auth logout -h "$HOST" 2>/dev/null; then
      ok "Logged out active account on $HOST"
    else
      warn "Logout returned an error (may already be logged out)."
    fi
  fi

  # Optional: clear bad keyring entry notes
  info "Config dir: ${XDG_CONFIG_HOME:-$HOME/.config}/gh"
  if [[ -f "${XDG_CONFIG_HOME:-$HOME/.config}/gh/hosts.yml" ]]; then
    info "hosts.yml still present (rewritten on next login)."
  fi
}

pick_git_protocol() {
  if [[ -n "$GIT_PROTOCOL" ]]; then
    echo "$GIT_PROTOCOL"
    return
  fi
  if [[ ! -t 0 ]]; then
    echo "ssh"
    return
  fi
  echo "" >&2
  warn "Git protocol for remotes:"
  echo "  1) ssh   (recommended if you already use git@github.com:...)" >&2
  echo "  2) https (uses gh credentials for git push/pull)" >&2
  read -r -p "Choose [1/2] (default 1): " choice
  case "${choice:-1}" in
    2|https|HTTPS) echo "https" ;;
    *) echo "ssh" ;;
  esac
}

do_login() {
  local proto
  proto="$(pick_git_protocol)"

  header "Login to $HOST as $ACCOUNT"
  info "Protocol: git=$proto"
  info "A browser window or one-time code prompt will open."
  echo
  info "Recommended scopes for normal dev work: repo, read:org, gist"
  info "(workflow may be needed if you manage GitHub Actions secrets)"
  echo

  # Interactive browser flow is the least painful on a desktop.
  # --web opens browser; --git-protocol sets push/pull style.
  # hostname -h for GH Enterprise would differ; we use github.com.
  gh auth login \
    -h "$HOST" \
    -p "$proto" \
    -w \
    -s "repo,read:org,gist"

  # Ensure git uses gh as credential helper when https
  if [[ "$proto" == "https" ]]; then
    gh auth setup-git -h "$HOST" || true
  fi

  ok "Login command finished."
}

verify_pr_capability() {
  header "5. PR capability smoke test"
  if gh api user -q .login >/dev/null 2>&1; then
    local user
    user="$(gh api user -q .login)"
    ok "Authenticated as: $user"
  else
    err "Still cannot call the API."
    return 1
  fi

  # Prefer a lightweight endpoint over creating anything
  if gh api rate_limit -q .resources.core.remaining >/dev/null 2>&1; then
    local rem
    rem="$(gh api rate_limit -q .resources.core.remaining 2>/dev/null || echo '?')"
    ok "API rate limit remaining: $rem"
  fi

  if git rev-parse --is-inside-work-tree >/dev/null 2>&1; then
    local remote
    remote="$(git remote get-url origin 2>/dev/null || true)"
    if [[ -n "$remote" ]]; then
      info "origin: $remote"
      if gh repo view --json nameWithOwner -q .nameWithOwner 2>/dev/null; then
        ok "→ gh can see this repository."
      else
        warn "→ gh cannot view this repo (permissions, wrong account, or remote)."
      fi
    fi
  fi
}

print_next_steps() {
  header "Next steps"
  cat <<EOF
Try:
  gh auth status
  gh pr list
  gh pr create --help

If login still fails:
  • Create a Personal Access Token (classic) with scopes: repo, read:org, gist
    https://github.com/settings/tokens
  • Then:  gh auth login -h $HOST -p ssh -w
    or paste the token when prompted (HTTPS + token flow).

SSH key issues (git@github.com):
  ssh -T git@github.com
  gh ssh-key list
  gh ssh-key add ~/.ssh/id_ed25519.pub --title "$(hostname)-$(date +%Y%m%d)"

Keyring / stale token (Linux desktop):
  Secret token may live in the GNOME keyring. After logout/login it should refresh.
  If you have both a "Default keyring" and "Login" keyring (gh password prompts):
    fix-gnome-keyring --status
    fix-gnome-keyring --migrate && fix-gnome-keyring --cleanup
  Worst case: remove hosts entry and re-login:
    rm -f ~/.config/gh/hosts.yml
    gh auth login -h $HOST -p ssh -w
EOF
}

main() {
  need_cmd gh

  case "$MODE" in
    check)
      if check_status; then
        verify_pr_capability || true
        ok "All good — no re-login needed."
        exit 0
      else
        err "Auth needs repair. Re-run: $0 --login"
        print_next_steps
        exit 1
      fi
      ;;
    login)
      check_status || true
      logout_stale
      do_login
      if check_status && verify_pr_capability; then
        ok "GitHub CLI auth is fixed."
        print_next_steps
        exit 0
      fi
      err "Login completed but verification failed."
      print_next_steps
      exit 1
      ;;
    interactive|*)
      header "GitHub CLI auth repair"
      if check_status && verify_pr_capability; then
        ok "Auth already works. Nothing to fix."
        print_next_steps
        exit 0
      fi

      echo
      warn "Auth check failed or incomplete."
      if [[ ! -t 0 ]]; then
        err "Non-interactive shell — re-run with: $0 --login"
        exit 1
      fi
      read -r -p "Logout and re-authenticate now? [Y/n] " ans
      case "${ans:-Y}" in
        n|N|no|NO)
          print_next_steps
          exit 1
          ;;
      esac
      logout_stale
      do_login
      if check_status && verify_pr_capability; then
        ok "GitHub CLI auth is fixed."
        print_next_steps
        exit 0
      fi
      err "Still broken after login."
      print_next_steps
      exit 1
      ;;
  esac
}

main
