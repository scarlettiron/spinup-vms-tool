#!/bin/sh
# Installed as /etc/xrdp/startwm.sh by bootstrap step 20-guest-tools.sh.
if [ -r /etc/default/locale ]; then
  . /etc/default/locale
  export LANG LANGUAGE
fi
export GNOME_SHELL_SESSION_MODE=ubuntu
export XDG_CURRENT_DESKTOP=ubuntu:GNOME
export XDG_SESSION_TYPE=x11
exec /etc/X11/Xsession
