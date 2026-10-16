#!/usr/bin/env bash
#Vmseed
#Licensed under the MIT License. See the LICENSE file in the project root.
#Copyright (c) 2026 Scarlett A. Scott (codescarlett)
#
#Contributors:
#Scarlett A. Scott (codescarlett)

#The status probe. `vmseed status` and `vmseed list --live` send this to a running
#guest over ssh and read back tab-separated "key<TAB>field..." lines. One round
#trip collects everything, so a slow guest is slow once. Every fact is optional:
#a missing file prints nothing rather than failing.
#The unit is a oneshot with RemainAfterExit, so systemd calls it "active" for
#good once it has finished; only "activating" means it is still running. The
#complete marker is the real answer, so it is checked first.
bootstrap_state="$(systemctl is-active vm-bootstrap.service 2>/dev/null)"
if [ -f /var/lib/vm-bootstrap/complete ]; then
    printf "bootstrap\tcomplete\t\n"
elif [ "$bootstrap_state" = "activating" ]; then
    printf "bootstrap\tprogress\t%s\n" "$(ls /var/lib/vm-bootstrap/*.done 2>/dev/null | wc -l)"
elif [ -f /var/log/vm-bootstrap-report.txt ]; then
    printf "bootstrap\tfailed\t%s\n" "$(sed -n "s/^ FAILED: //p" /var/log/vm-bootstrap-report.txt | head -1)"
else
    printf "bootstrap\tprogress\t%s\n" "$(ls /var/lib/vm-bootstrap/*.done 2>/dev/null | wc -l)"
fi

. /etc/os-release 2>/dev/null
printf "os\t%s\n" "${PRETTY_NAME:-unknown}"
printf "kernel\t%s\n" "$(uname -r)"
printf "guestuptime\t%s\n" "$(uptime -p 2>/dev/null | sed "s/^up //")"

for u in $(getent passwd | awk -F: "\$3>=1000 && \$3<65534 {print \$1}"); do
    # GECOS is comma separated and usually only the first part is a name, so an
    # account with no real name gives ",,," rather than nothing. Take field one.
    printf "user\t%s\t%s\t%s\n" "$u" \
        "$(getent passwd "$u" | cut -d: -f5 | cut -d, -f1)" \
        "$(id -nG "$u" 2>/dev/null)"
done

for mp in / $(awk -F"\t" "!/^#/ && NF>2 {print \$3}" /etc/vm-bootstrap.disks 2>/dev/null); do
    [ -d "$mp" ] || continue
    set -- $(df -P -B1 "$mp" 2>/dev/null | tail -1)
    [ -n "$2" ] || continue
    printf "df\t%s\t%s\t%s\t%s\n" "$mp" "$2" "$3" "$5"
done

if [ -s /var/lib/vm-bootstrap/database ]; then
    printf "database\t%s\t%s\t%s\t%s\t%s\t%s\n" \
        "$(cut -f1 /var/lib/vm-bootstrap/database | head -1)" \
        "$(cut -f2 /var/lib/vm-bootstrap/database | head -1)" \
        "$(cut -f3 /var/lib/vm-bootstrap/database | head -1)" \
        "$(cut -f4 /var/lib/vm-bootstrap/database | head -1)" \
        "$(cut -f5 /var/lib/vm-bootstrap/database | head -1)" \
        "$(cut -f6 /var/lib/vm-bootstrap/database | head -1)"
fi

if [ -s /var/lib/vm-bootstrap/adminer ]; then
    printf "adminer\t%s\t%s\t%s\t%s\n" \
        "$(cut -f1 /var/lib/vm-bootstrap/adminer | head -1)" \
        "$(cut -f2 /var/lib/vm-bootstrap/adminer | head -1)" \
        "$(cut -f3 /var/lib/vm-bootstrap/adminer | head -1)" \
        "$(cut -f4 /var/lib/vm-bootstrap/adminer | head -1)"
fi

if [ -f /var/log/vm-post-start.log ]; then
    printf "hooks\t%s\t%s\n" \
        "$(wc -l < /var/log/vm-post-start.log | tr -d ' ')" \
        "$(systemctl is-enabled vm-post-start.service 2>/dev/null || echo once)"
fi

if [ -s /var/lib/vm-bootstrap/github-auth ]; then
    printf "github\t%s\t%s\n" \
        "$(cut -f1 /var/lib/vm-bootstrap/github-auth | head -1)" \
        "$(cut -f2 /var/lib/vm-bootstrap/github-auth | head -1)"
fi
if [ -s /var/lib/vm-bootstrap/browser ]; then
    printf "browser\t%s\n" "$(head -1 /var/lib/vm-bootstrap/browser)"
fi
printf "ip\t%s\n" "$(hostname -I 2>/dev/null | awk '{print $1}')"
