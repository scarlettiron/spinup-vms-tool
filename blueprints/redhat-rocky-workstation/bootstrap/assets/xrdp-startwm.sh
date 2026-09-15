#!/bin/sh
# Scarlett A. Scott
# Created 2026-09-15

# Installed as /etc/xrdp/startwm.sh by bootstrap step 20-hyperv.sh.
if [ -r /etc/locale.conf ]; then
  . /etc/locale.conf
  export LANG
fi
# classic rather than the default session: GNOME Shell's normal mode wants
# graphics acceleration that the Hyper-V framebuffer does not provide, and
# gnome-classic-session is in Rocky's AppStream.
export GNOME_SHELL_SESSION_MODE=classic
export XDG_CURRENT_DESKTOP=GNOME
export XDG_SESSION_TYPE=x11
exec /etc/X11/xinit/Xsession
