#!/usr/bin/env bash
#
# Installed as /usr/local/sbin/vm-bootstrap and run once by
# vm-bootstrap.service on first boot. Safe to re-run by hand:
#
#   sudo vm-bootstrap                # run any steps that have not completed
#   sudo vm-bootstrap --force        # re-run every step
#   sudo vm-bootstrap --only 30-node # re-run one step
#   sudo vm-bootstrap --list         # show steps and their state
#
set -euo pipefail

ROOT="${VM_BOOTSTRAP_ROOT:-/usr/local/lib/vm-bootstrap}"
STATE_DIR=/var/lib/vm-bootstrap
LOG=/var/log/vm-bootstrap.log
REPORT=/var/log/vm-bootstrap-report.txt
ENV_FILE=/etc/vm-bootstrap.env

FORCE=0
ONLY=""
LIST=0
while [[ $# -gt 0 ]]; do
    case "$1" in
        --force) FORCE=1 ;;
        --only)  ONLY="${2:?--only needs a step name}"; shift ;;
        --list)  LIST=1 ;;
        -h|--help) sed -n '2,12p' "$0"; exit 0 ;;
        *) echo "unknown argument: $1" >&2; exit 2 ;;
    esac
    shift
done

[[ $EUID -eq 0 ]] || { echo "vm-bootstrap must run as root" >&2; exit 1; }

mkdir -p "$STATE_DIR"
steps=()
while IFS= read -r f; do steps+=("$f"); done < <(find "$ROOT" -maxdepth 1 -name '[0-9][0-9]-*.sh' | sort)

if [[ $LIST -eq 1 ]]; then
    for s in "${steps[@]}"; do
        name="$(basename "$s" .sh)"
        if [[ -f "$STATE_DIR/$name.done" ]]; then state=done; else state=pending; fi
        printf '%-24s %s\n' "$name" "$state"
    done
    exit 0
fi

exec > >(tee -a "$LOG") 2>&1

echo "=============================================================="
echo " vm-bootstrap starting $(date -Is)"
echo "=============================================================="

# shellcheck source=/dev/null
[[ -f "$ENV_FILE" ]] && set -a && . "$ENV_FILE" && set +a
export VM_BOOTSTRAP_ROOT="$ROOT"

# Wait for the package manager to be free. Something else is usually holding it
# during the first minutes of uptime: cloud-init and unattended-upgrades on
# Ubuntu, dnf-makecache on Fedora.
for lock in /var/lib/dpkg/lock-frontend /var/lib/rpm/.rpm.lock /var/cache/dnf/metadata_lock.pid; do
    [[ -e "$lock" ]] || continue
    for _ in $(seq 1 60); do
        fuser "$lock" >/dev/null 2>&1 || break
        echo "waiting for $lock..."
        sleep 5
    done
done

failed=()
# One "name<TAB>status<TAB>seconds" line per step attempted, for the report below.
results=()
run_started=$SECONDS

for step in "${steps[@]}"; do
    name="$(basename "$step" .sh)"
    [[ -z "$ONLY" || "$ONLY" == "$name" ]] || continue

    if [[ $FORCE -eq 0 && -z "$ONLY" && -f "$STATE_DIR/$name.done" ]]; then
        echo "--- $name: already complete, skipping"
        results+=("$name	skipped	0")
        continue
    fi

    echo "--------------------------------------------------------------"
    echo "--- $name: start"
    start=$SECONDS
    if ( set -euo pipefail; . "$ROOT/_lib.sh"; . "$step" ); then
        touch "$STATE_DIR/$name.done"
        elapsed=$((SECONDS - start))
        echo "--- $name: ok (${elapsed}s)"
        results+=("$name	ok	$elapsed")
    else
        rc=$?
        elapsed=$((SECONDS - start))
        failed+=("$name")
        echo "--- $name: FAILED (exit $rc, ${elapsed}s) - continuing so one bad step"
        echo "    cannot strand the whole machine. Re-run: sudo vm-bootstrap --only $name"
        results+=("$name	FAILED	$elapsed")
    fi
done

# ---------------------------------------------------------------------- report
#
# Written to its own file as well as the log. Someone troubleshooting this VM in a
# week wants one short page saying what ran and what broke, not 4000 lines of dnf
# output - and the full log is right there when they do want it.
total=$((SECONDS - run_started))
ok_count=0; failed_count=0; skipped_count=0

{
    echo "=============================================================="
    echo " vm-bootstrap report"
    echo " $(date -Is)   host: $(hostnamectl --static 2>/dev/null || hostname)"
    echo "=============================================================="
    echo
    printf ' %-22s %-8s %s\n' "STEP" "STATUS" "TIME"
    printf ' %-22s %-8s %s\n' "----------------------" "--------" "-----"
    for entry in "${results[@]}"; do
        IFS=$'\t' read -r r_name r_status r_secs <<< "$entry"
        case "$r_status" in
            ok)      ok_count=$((ok_count + 1)) ;;
            FAILED)  failed_count=$((failed_count + 1)) ;;
            skipped) skipped_count=$((skipped_count + 1)) ;;
        esac
        if [[ "$r_status" == "skipped" ]]; then
            printf ' %-22s %-8s %s\n' "$r_name" "$r_status" "already done"
        else
            printf ' %-22s %-8s %ss\n' "$r_name" "$r_status" "$r_secs"
        fi
    done
    echo
    echo " ${#results[@]} step(s): $ok_count ok, $failed_count failed, $skipped_count skipped   total ${total}s"

    if [[ ${#failed[@]} -gt 0 ]]; then
        echo
        echo " FAILED: ${failed[*]}"
        echo " Re-run just those:"
        for f in "${failed[@]}"; do
            echo "     sudo vm-bootstrap --only $f --force"
        done
    fi

    echo
    echo " Logs on this machine:"
    echo "     $LOG"
    echo "         every line of output from this run, in full"
    echo "     $REPORT"
    echo "         this report on its own"
    echo "     journalctl -u vm-bootstrap.service"
    echo "         the same output as the service saw it"
    # The installer's own logs, which is where to look when the problem predates
    # bootstrap. Paths differ per installer, so only mention what exists.
    for installer_log in /var/log/cloud-init.log /var/log/cloud-init-output.log \
                         /var/log/anaconda/anaconda.log /var/log/vm-bootstrap-post.log; do
        [[ -f "$installer_log" ]] && echo "     $installer_log"
    done
    echo
    echo " State:  $STATE_DIR   (one .done stamp per completed step)"
    echo "=============================================================="
} | tee "$REPORT"

if [[ ${#failed[@]} -eq 0 ]]; then
    touch "$STATE_DIR/complete"
fi

[[ ${#failed[@]} -eq 0 ]]
