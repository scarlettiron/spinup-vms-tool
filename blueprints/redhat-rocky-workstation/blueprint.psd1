# Scarlett A. Scott
# Created 2026-09-15

<#
    Blueprint: redhat-rocky-workstation

    Rocky Linux with the GNOME Workstation environment, installed unattended from
    the boot (netinst) image and then provisioned as a dev workstation.

    The name says both halves on purpose. 'redhat' is the family - what the
    machine is compatible with, and what you would search the repo for. 'rocky'
    is the distribution actually installed, so nobody has to open this file to
    find out which one it is. A future redhat-alma-workstation or a real
    redhat-rhel-workstation would sit beside it and read the same way.

    Why Rocky rather than Red Hat Enterprise Linux itself: RHEL's ISOs are behind
    a Red Hat account, so nothing in this repo can fetch or verify one without a
    subscription, and a fresh RHEL install has no package repositories at all
    until subscription-manager has registered it. Rocky is a binary-compatible
    rebuild of the same source with public media and public repositories, so the
    ordinary "download, verify, install" path works. If you develop against RHEL
    servers, this is the machine that matches them.

    Differences from fedora-workstation that actually matter, given both are dnf:

      EPEL is mandatory       RHEL ships a deliberately small repository. xrdp,
                              xorgxrdp, nss-mdns, ripgrep, fd-find, fzf, pipx,
                              htop, openvpn, openconnect, sshuttle, remmina, gh
                              and the Fira Code fonts are all EPEL packages here.
                              Losing xrdp means losing Enhanced Session, so
                              01-base.sh calls enable_epel before anything else.
      BaseOS and AppStream    Two repositories, not one, so the kickstart needs a
                              'repo' line beside its 'url' line.
      @development            the comps group Fedora calls @development-tools.
      vim-enhanced            Fedora's 'vim' metapackage does not exist here.
      AppStream modules       'dnf module disable postgresql' before PGDG's
                              packages will win. Fedora has no modules.
      no direnv, no mkcert    not packaged for EL9 anywhere, including EPEL and
                              CRB. Both are single static Go binaries, so
                              01-base.sh and 33-ddev.sh fetch the releases.

    Everything else - kickstart on OEMDRV, wheel, SELinux enforcing, firewalld,
    the polkit .rules format, /etc/X11/xinit/Xsession - is shared with Fedora,
    which is why the two blueprints run the same bootstrap steps.
#>
@{
    Name        = 'redhat-rocky-workstation'
    Description = 'Rocky Linux + GNOME Workstation, provisioned as a dev workstation'

    Os = @{
        Family  = 'linux'
        Vendor  = 'rocky'

        # The major version. Rocky publishes point releases (9.8 and so on) under
        # the major version's directory, and the blueprint follows whichever is
        # current: the checksum file is re-read every build, so a point release
        # is picked up without editing anything here.
        Version = '9'

        # Deliberately '9' only, and not '\d+'.
        #
        # Rocky 10 exists and installs, but EPEL 10 does not carry xrdp or
        # xorgxrdp - checked against two mirrors - and without those there is no
        # Enhanced Session Mode, which is the whole reason the desktop is usable
        # from a Hyper-V window. Rather than let that fail ninety minutes into a
        # build, 10 is refused here with a message that says which version to
        # use. Widen this the day EPEL 10 ships xrdp.
        VersionPattern = '^9$'
        SupportPolicy  = 'RHEL 9 lifecycle: full support to 2027, maintenance to May 2032'
    }

    Media = @{
        # Rocky publishes a stable '-latest-' name alongside the point-release
        # one, and a small per-ISO CHECKSUM beside each. The per-ISO file is used
        # rather than the directory's combined CHECKSUM for two reasons: it holds
        # exactly one entry, so there is nothing to pick between, and the
        # combined file's entries do not sort the way you would hope. Descending
        # order puts 'Rocky-9-latest' above 'Rocky-9.8', and once Rocky reaches
        # 9.10 it would sort below 9.8 as well.
        #
        # The wildcard is what makes this survive a version bump: the file is
        # found by scraping the release directory, so no build number is
        # hard-coded. A point release changes the file behind the stable name,
        # the cached ISO then fails verification, and Resolve-DevVMIso downloads
        # the new one by itself.
        IsoBaseUrl     = 'https://download.rockylinux.org/pub/rocky/{Release}/isos/x86_64/'
        IsoNamePattern = '*-boot.iso'
        # BSD-style tags, the same format Fedora publishes:
        #   SHA256 (Rocky-9-latest-x86_64-boot.iso) = <hash>
        ChecksumFile   = 'Rocky-*-latest-x86_64-boot.iso.CHECKSUM'
    }

    Provisioning = @{
        Method        = 'kickstart-oemdrv'   # FAT32 disk labelled OEMDRV
        SeedLabel     = 'OEMDRV'
        SeedFileName  = 'ks.cfg'
        SeedTemplate  = 'seed/ks.cfg.tmpl'
        # No MetaTemplate: kickstart has no equivalent of cloud-init's meta-data.

        # The bootstrap steps come from blueprints/bootstrap. Nothing here
        # overrides a step: every Rocky-versus-Fedora difference is a name or a
        # repository, which the shared steps handle by branching on $OS_FAMILY.
        # Only the two xrdp assets are blueprint-local.
        BootstrapDirs = @('../bootstrap', 'bootstrap')
        BootstrapRoot = '/usr/local/lib/vm-bootstrap'

        # Anaconda's target disk. Safe because the host attaches the data disks
        # only after the install (see Add-DevVMDataDisks): during it the system
        # disk is the only real disk present.
        SystemDisk = 'sda'

        # Where packages come from, since the boot image carries only the
        # installer. Unlike Fedora this takes two lines: RHEL splits the OS
        # across BaseOS (the core) and AppStream (everything with a lifecycle of
        # its own, GNOME included), and an install source naming only BaseOS
        # fails on the first AppStream package.
        #
        # {Release} is substituted at render time, so a version bump needs no
        # edit here. The mirrorlist is keyed on the major version - 'BaseOS-9' -
        # and returns nearby mirrors of the current point release.
        InstallSource = @'
url --mirrorlist="https://mirrors.rockylinux.org/mirrorlist?arch=x86_64&repo=BaseOS-{Release}"
repo --name="AppStream" --mirrorlist="https://mirrors.rockylinux.org/mirrorlist?arch=x86_64&repo=AppStream-{Release}"
'@

        PowerOffAfterInstall  = $true
        InstallTimeoutMinutes = 90
    }

    Firmware = @{
        Generation          = 2
        # Rocky's shim is signed by the Microsoft UEFI CA, same as Fedora's and
        # Ubuntu's, so Secure Boot works with the stock template.
        SecureBootTemplate  = 'MicrosoftUEFICertificateAuthority'
        EnableTpm           = $false
    }

    # Laid down by the installer. @^ selects an environment group, @ a plain
    # group; everything else is a package. All of these are in BaseOS or
    # AppStream: EPEL is not available during the install, so anything from it
    # waits for bootstrap.
    Packages = @(
        '@^workstation-product-environment'
        '@standard'
        'openssh-server'
        'curl'
        'wget'
        'ca-certificates'
        'gnupg2'
        'git'
        'jq'
        'unzip'
        'hyperv-daemons'
        'python3'
    )

    Defaults = @{
        # Sized like the Fedora workstation: a GNOME desktop pulled from the
        # mirror rather than off the ISO.
        VM = @{
            MemoryStartupGB = 12
            MemoryMaximumGB = 16
        }
        Bootstrap = @{
            # The desktop group, named the way THIS OS names it. Rocky carries
            # the same comps group as Fedora, verified against the AppStream
            # repository's comps.xml. Normally a no-op, since the kickstart has
            # already installed the environment by the time 10-desktop.sh runs.
            DesktopPackage = '@workstation-product-environment'
        }
    }
}
