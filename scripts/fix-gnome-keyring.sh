#!/usr/bin/env bash
# fix-gnome-keyring.sh — consolidate GNOME keyrings onto a single "login" keyring
#
# Typical mess on Ubuntu/Zorin:
#   • "Default keyring"  — often passwordless / auto-unlocked, holds browser secrets
#   • "Login"            — PAM-unlocked at desktop login, holds gh / app tokens
#   • ~/.local/share/keyrings/default  points at Default_keyring
#
# Result: some apps “just work”, gh prompts for a password, or tokens land in
# different places. Fix: one keyring ("login"), set as default, unlocked at login.
#
# Usage:
#   ./scripts/fix-gnome-keyring.sh            # interactive
#   ./scripts/fix-gnome-keyring.sh --status   # diagnose only
#   ./scripts/fix-gnome-keyring.sh --migrate  # move secrets → login, set default
#   ./scripts/fix-gnome-keyring.sh --cleanup  # after migrate: remove Default_keyring
#
# Safe defaults: never prints secret values. Always dry-runs item labels first.

set -euo pipefail

MODE="interactive"
KEYRINGS_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/keyrings"
DEFAULT_PTR="$KEYRINGS_DIR/default"

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
  sed -n '2,20p' "$0" | sed 's/^# \?//'
  exit 0
}

for arg in "$@"; do
  case "$arg" in
    -h|--help)    usage ;;
    --status)     MODE="status" ;;
    --migrate)    MODE="migrate" ;;
    --cleanup)    MODE="cleanup" ;;
    *) err "Unknown option: $arg"; usage ;;
  esac
done

need_python_dbus() {
  if ! python3 -c 'import dbus' 2>/dev/null; then
    err "python3-dbus is required (Secret Service API)."
    info "Install:  sudo apt install python3-dbus"
    exit 1
  fi
}

# ---------- diagnostics (bash) ----------
show_files() {
  header "Keyring files ($KEYRINGS_DIR)"
  if [[ ! -d "$KEYRINGS_DIR" ]]; then
    warn "No keyrings directory yet."
    return
  fi
  ls -la "$KEYRINGS_DIR"
  echo
  if [[ -f "$DEFAULT_PTR" ]]; then
    info "default pointer → '$(cat "$DEFAULT_PTR")'"
  else
    warn "No 'default' pointer file (gnome-keyring will guess)."
  fi
  echo
  if command -v file >/dev/null; then
    file "$KEYRINGS_DIR"/* 2>/dev/null || true
  fi
}

# ---------- Secret Service helpers (python) ----------
# Shared python blob used by status / migrate / cleanup.
run_ss() {
  local action="$1"
  ACTION="$action" python3 - <<'PY'
import os, sys, time
import dbus
from dbus.mainloop.glib import DBusGMainLoop

# Optional GLib for unlock prompts
try:
    import gi
    gi.require_version('GLib', '2.0')
    from gi.repository import GLib
    DBusGMainLoop(set_as_default=True)
    HAS_GLIB = True
except Exception:
    HAS_GLIB = False

ACTION = os.environ.get("ACTION", "status")

bus = dbus.SessionBus()
service = bus.get_object("org.freedesktop.secrets", "/org/freedesktop/secrets")
sprops = dbus.Interface(service, "org.freedesktop.DBus.Properties")
siface = dbus.Interface(service, "org.freedesktop.Secret.Service")

def coll_props(path):
    obj = bus.get_object("org.freedesktop.secrets", path)
    return dbus.Interface(obj, "org.freedesktop.DBus.Properties"), obj

def list_collections():
    out = []
    for path in sprops.Get("org.freedesktop.Secret.Service", "Collections"):
        p, _ = coll_props(path)
        label = str(p.Get("org.freedesktop.Secret.Collection", "Label") or "")
        locked = bool(p.Get("org.freedesktop.Secret.Collection", "Locked"))
        items = list(p.Get("org.freedesktop.Secret.Collection", "Items"))
        # basename-ish id
        cid = str(path).rsplit("/", 1)[-1]
        out.append({"path": str(path), "id": cid, "label": label, "locked": locked, "items": items})
    return out

def find_coll(collections, want):
    """want: 'login' | 'default_keyring' | label match"""
    for c in collections:
        cid = c["id"].lower().replace("_5f", "_")  # dbus encodes '_' as _5f sometimes
        label = c["label"].lower()
        if want == "login" and (cid == "login" or label == "login"):
            return c
        if want == "default_keyring" and (
            "default" in cid or label in ("default keyring", "default")
        ) and cid != "login" and "session" not in cid:
            return c
    return None

def unlock(paths):
    paths = [dbus.ObjectPath(p) for p in paths]
    # Unlock may prompt; returns (unlocked, prompt)
    unlocked, prompt = siface.Unlock(paths)
    if prompt and str(prompt) != "/":
        print(f"  (unlock prompt: {prompt})", flush=True)
        pobj = bus.get_object("org.freedesktop.secrets", prompt)
        piface = dbus.Interface(pobj, "org.freedesktop.Secret.Prompt")
        if HAS_GLIB:
            loop = GLib.MainLoop()
            done = {"ok": False}

            def completed(dismissed, result):
                done["ok"] = not bool(dismissed)
                loop.quit()

            piface.connect_to_signal("Completed", completed)
            piface.Prompt("")  # window-id empty
            # timeout 120s
            GLib.timeout_add_seconds(120, loop.quit)
            loop.run()
            if not done["ok"]:
                print("ERROR: unlock dismissed or timed out", file=sys.stderr)
                return False
        else:
            piface.Prompt("")
            time.sleep(2)
    return True

def open_session():
    # plain session: secret bytes returned in D-Bus message (local only)
    _out, session = siface.OpenSession("plain", dbus.String("", variant_level=1))
    return session

def item_meta(item_path):
    p, _ = coll_props(item_path)
    label = str(p.Get("org.freedesktop.Secret.Item", "Label") or "")
    attrs = dict(p.Get("org.freedesktop.Secret.Item", "Attributes") or {})
    # stringify attrs safely
    safe = {str(k): str(v) for k, v in attrs.items()}
    return label, safe

def get_secret_bytes(item_path, session):
    obj = bus.get_object("org.freedesktop.secrets", item_path)
    iiface = dbus.Interface(obj, "org.freedesktop.Secret.Item")
    secret = iiface.GetSecret(session)
    # (session_path, parameters, value, content_type)
    value = bytes(secret[2])
    content_type = str(secret[3]) if secret[3] else "text/plain"
    return value, content_type

def create_item(coll_path, label, attrs, value: bytes, content_type: str, session):
    obj = bus.get_object("org.freedesktop.secrets", coll_path)
    ciface = dbus.Interface(obj, "org.freedesktop.Secret.Collection")
    props = dbus.Dictionary(
        {
            "org.freedesktop.Secret.Item.Label": label,
            "org.freedesktop.Secret.Item.Attributes": dbus.Dictionary(
                {str(k): str(v) for k, v in attrs.items()}, signature="ss"
            ),
        },
        signature="sv",
    )
    secret = (
        session,
        dbus.ByteArray(b""),  # parameters for plain
        dbus.ByteArray(value),
        content_type or "text/plain",
    )
    item_path, prompt = ciface.CreateItem(props, secret, True)  # replace=True
    if prompt and str(prompt) != "/":
        pobj = bus.get_object("org.freedesktop.secrets", prompt)
        dbus.Interface(pobj, "org.freedesktop.Secret.Prompt").Prompt("")
        time.sleep(0.5)
    return str(item_path)

def delete_item(item_path):
    obj = bus.get_object("org.freedesktop.secrets", item_path)
    iiface = dbus.Interface(obj, "org.freedesktop.Secret.Item")
    prompt = iiface.Delete()
    if prompt and str(prompt) != "/":
        pobj = bus.get_object("org.freedesktop.secrets", prompt)
        dbus.Interface(pobj, "org.freedesktop.Secret.Prompt").Prompt("")
        time.sleep(0.3)

def delete_collection(coll_path):
    obj = bus.get_object("org.freedesktop.secrets", coll_path)
    ciface = dbus.Interface(obj, "org.freedesktop.Secret.Collection")
    prompt = ciface.Delete()
    if prompt and str(prompt) != "/":
        pobj = bus.get_object("org.freedesktop.secrets", prompt)
        dbus.Interface(pobj, "org.freedesktop.Secret.Prompt").Prompt("")
        time.sleep(0.5)

def set_default_alias(coll_path):
    # Make this collection the default for new secrets
    siface.SetAlias("default", dbus.ObjectPath(coll_path))

def print_status():
    cols = list_collections()
    print("Secret Service collections:")
    for c in cols:
        print(f"  • {c['label'] or '(unnamed)'}  id={c['id']}  locked={c['locked']}  items={len(c['items'])}")
        for i, ip in enumerate(c["items"][:12]):
            label, attrs = item_meta(ip)
            # highlight interesting services
            svc = attrs.get("service", attrs.get("application", ""))
            print(f"      [{i}] {label!r}  service/app={svc!r}")
        if len(c["items"]) > 12:
            print(f"      … +{len(c['items'])-12} more")
    login = find_coll(cols, "login")
    dflt = find_coll(cols, "default_keyring")
    print()
    if login:
        print(f"login keyring:   {login['path']}  locked={login['locked']} items={len(login['items'])}")
    else:
        print("login keyring:   NOT FOUND")
    if dflt:
        print(f"Default keyring: {dflt['path']}  locked={dflt['locked']} items={len(dflt['items'])}")
    else:
        print("Default keyring: not present (good — already consolidated?)")
    # alias
    try:
        aliases = sprops.Get("org.freedesktop.Secret.Service", "Aliases")
        # may be a dict path->alias or list depending on impl
        print(f"aliases raw: {aliases}")
    except Exception as e:
        print(f"(could not read aliases: {e})")

def do_migrate():
    cols = list_collections()
    login = find_coll(cols, "login")
    dflt = find_coll(cols, "default_keyring")
    if not login:
        print("ERROR: no Login keyring. Log into GNOME once, or create one in Seahorse (Passwords and Keys).", file=sys.stderr)
        sys.exit(2)
    # Unlock both
    to_unlock = [login["path"]]
    if dflt:
        to_unlock.append(dflt["path"])
    print("Unlocking keyrings (enter password if prompted)…")
    if not unlock(to_unlock):
        sys.exit(3)
    # re-fetch after unlock
    cols = list_collections()
    login = find_coll(cols, "login")
    dflt = find_coll(cols, "default_keyring")

    session = open_session()
    moved = 0
    skipped = 0

    if dflt and dflt["items"]:
        print(f"Migrating {len(dflt['items'])} item(s) from Default keyring → Login…")
        for ip in list(dflt["items"]):
            label, attrs = item_meta(ip)
            try:
                value, ctype = get_secret_bytes(ip, session)
            except Exception as e:
                print(f"  SKIP (read failed) {label!r}: {e}")
                skipped += 1
                continue
            try:
                create_item(login["path"], label, attrs, value, ctype, session)
                delete_item(ip)
                print(f"  moved: {label!r}  attrs_keys={sorted(attrs.keys())}")
                moved += 1
            except Exception as e:
                print(f"  SKIP (write failed) {label!r}: {e}")
                skipped += 1
    else:
        print("No Default-keyring items to migrate (or Default keyring absent).")

    # Dedupe gh tokens: prefer username=maplepreneur, drop empty username if both exist
    cols = list_collections()
    login = find_coll(cols, "login")
    gh_items = []
    for ip in login["items"]:
        label, attrs = item_meta(ip)
        if attrs.get("service") == "gh:github.com":
            gh_items.append((ip, label, attrs))
    if len(gh_items) > 1:
        print(f"Found {len(gh_items)} gh:github.com secrets; preferring named account…")
        preferred = [x for x in gh_items if x[2].get("username")]
        drop = [x for x in gh_items if not x[2].get("username")]
        if preferred and drop:
            for ip, label, attrs in drop:
                try:
                    delete_item(ip)
                    print(f"  removed duplicate: {label!r} (empty username)")
                except Exception as e:
                    print(f"  could not remove {label!r}: {e}")

    # Point default alias + file at login
    print("Setting default collection → Login…")
    set_default_alias(login["path"])
    # Also write the pointer file gnome-keyring reads
    keyrings = os.path.expanduser("~/.local/share/keyrings")
    os.makedirs(keyrings, mode=0o700, exist_ok=True)
    ptr = os.path.join(keyrings, "default")
    with open(ptr, "w", encoding="utf-8") as f:
        f.write("login")
    os.chmod(ptr, 0o600)
    print(f"Wrote {ptr} → login")
    print(f"Done. moved={moved} skipped={skipped}")
    print("Next: run with --cleanup to delete the empty Default keyring (optional).")
    print("Then re-login or:  killall gnome-keyring-daemon  (session will restart it)")
    print("Verify:  gh auth status")

def do_cleanup():
    cols = list_collections()
    dflt = find_coll(cols, "default_keyring")
    login = find_coll(cols, "login")
    if not dflt:
        print("Default keyring already gone.")
        return
    if dflt["items"]:
        print(f"ERROR: Default keyring still has {len(dflt['items'])} item(s). Run --migrate first.", file=sys.stderr)
        sys.exit(4)
    if not login:
        print("ERROR: refusing to delete Default without a Login keyring.", file=sys.stderr)
        sys.exit(4)
    # ensure default alias is login
    set_default_alias(login["path"])
    print(f"Deleting empty Default keyring at {dflt['path']}…")
    unlock([dflt["path"]])
    delete_collection(dflt["path"])
    # remove leftover files if daemon left them
    kd = os.path.expanduser("~/.local/share/keyrings")
    for name in ("Default_keyring.keyring", "default.keyring"):
        p = os.path.join(kd, name)
        if os.path.isfile(p):
            os.remove(p)
            print(f"  removed file {p}")
    # ensure pointer
    with open(os.path.join(kd, "default"), "w", encoding="utf-8") as f:
        f.write("login")
    print("Cleanup complete. Prefer a full logout/login so PAM unlocks Login at session start.")

if ACTION == "status":
    print_status()
elif ACTION == "migrate":
    do_migrate()
elif ACTION == "cleanup":
    do_cleanup()
else:
    print(f"unknown ACTION={ACTION}", file=sys.stderr)
    sys.exit(1)
PY
}

print_pam_notes() {
  header "Login unlock (PAM)"
  if grep -Rqs 'pam_gnome_keyring' /etc/pam.d/gdm-password 2>/dev/null; then
    ok "gdm-password includes pam_gnome_keyring (good)."
  else
    warn "Could not confirm pam_gnome_keyring in gdm-password."
  fi
  cat <<EOF

How it should work after consolidation:
  • Desktop login password unlocks the "Login" keyring via PAM
  • Apps (Brave, gh, …) store secrets only there
  • No second password prompt for a "Default keyring"

If Login still asks for a password after login:
  • Login keyring password must match your user password
  • Fix in Seahorse: right-click "Login" → Change Password → set to your
    account password (or leave empty only if you accept less security)
  • Or: log out fully and log back in (not just lock screen)
EOF
}

print_status_all() {
  show_files
  header "Secret Service"
  need_python_dbus
  run_ss status
  print_pam_notes
  header "gh"
  if command -v gh >/dev/null; then
    gh auth status 2>&1 || true
  else
    warn "gh not installed"
  fi
}

do_migrate() {
  need_python_dbus
  header "Migrate → Login keyring"
  warn "You may get a keyring password prompt (for Login or Default)."
  run_ss migrate
  header "Status after migrate"
  run_ss status
  print_pam_notes
}

do_cleanup() {
  need_python_dbus
  header "Remove empty Default keyring"
  run_ss cleanup
  show_files
  run_ss status
}

main() {
  case "$MODE" in
    status) print_status_all ;;
    migrate) do_migrate ;;
    cleanup) do_cleanup ;;
    interactive)
      print_status_all
      echo
      if [[ ! -t 0 ]]; then
        info "Non-interactive: re-run with --migrate when ready."
        exit 0
      fi
      read -r -p "Migrate all secrets into Login and make it the only default? [y/N] " ans
      case "${ans:-N}" in
        y|Y|yes|YES)
          do_migrate
          echo
          read -r -p "Delete empty Default keyring now? [y/N] " ans2
          case "${ans2:-N}" in
            y|Y|yes|YES) do_cleanup ;;
            *) info "Skipped cleanup. Run later: $0 --cleanup" ;;
          esac
          ok "Done. Log out and back in once so Login unlocks with your session."
          info "Then:  fix-gh-auth --check   &&   gh auth status"
          ;;
        *)
          info "No changes. When ready:  $0 --migrate && $0 --cleanup"
          ;;
      esac
      ;;
  esac
}

main
