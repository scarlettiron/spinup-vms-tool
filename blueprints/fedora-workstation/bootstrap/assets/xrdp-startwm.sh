#!/bin/sh
# Installed as /etc/xrdp/startwm.sh by bootstrap step 20-guest-tools.sh.
if [ -r /etc/locale.conf ]; then
  . /etc/locale.conf
  export LANG
fi
export GNOME_SHELL_SESSION_MODE=classic
export XDG_CURRENT_DESKTOP=GNOME
export XDG_SESSION_TYPE=x11
exec /etc/X11/xinit/Xsession
