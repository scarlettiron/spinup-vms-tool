<#
    Blueprint: redhat-rhel-workstation

    Red Hat Enterprise Linux itself, with the GNOME Workstation environment,
    installed unattended from the binary DVD and then provisioned as a dev
    workstation.

    This is the real thing, not a rebuild. Use it when you need RHEL for support,
    compliance, or because you are reproducing a customer's exact platform. If
    you only need "something that behaves like a Red Hat server", prefer
    redhat-rocky-workstation: it is binary-compatible, free, needs no account,
    and downloads its own media.

    TWO THINGS ARE DIFFERENT FROM EVERY OTHER BLUEPRINT HERE, and both come from
    RHEL being a subscription product rather than anything technical.

    1. THE ISO CANNOT BE DOWNLOADED. Red Hat's media is behind an account, so
       nothing in this repo can fetch or checksum it. You download it once, by
       hand, and point Iso.Path at it. A no-cost Red Hat Developer subscription
       is enough:

           https://developers.redhat.com/products/rhel/download

       Take the BINARY DVD, not the boot ISO. The boot ISO carries the installer
       and no packages, and it expects to pull them from Red Hat's CDN, which
       needs the machine entitled before the install has even finished. The DVD
       carries BaseOS and AppStream, so the install completes with no network
       and no subscription at all.

    2. A FRESH RHEL HAS NO REPOSITORIES. Until subscription-manager registers the
       machine, dnf can install nothing that is not already on the DVD. That is
       not a soft limit: it costs EPEL, therefore xrdp, therefore Enhanced
       Session. Put an activation key in your LOCAL config:

           redHat:
             activationKey:  your-key
             organizationId: '12345678'

       Get both from console.redhat.com, under Connector then Activation Keys.
       An activation key is the credential built for this: it carries no
       password and can be revoked on its own. Username and password also work
       and are a worse idea on a seed disk.

       Build without either and the machine still comes up, with everything the
       DVD provides. The plan says so at render time, and bootstrap says so
       again, rather than letting it look like a failure later.

    Everything else is shared with redhat-rocky-workstation, because below the
    subscription layer they are the same operating system: kickstart on OEMDRV,
    dnf, wheel, SELinux enforcing, firewalld, EPEL for the dev tooling, the same
    polkit and xrdp assets.
#>
@{
    Name        = 'redhat-rhel-workstation'
    Description = 'Red Hat Enterprise Linux + GNOME Workstation (needs your own ISO and a subscription)'

    Os = @{
        Family  = 'linux'
        Vendor  = 'rhel'

        # A full minor release, not a bare major.
        #
        # Rocky tracks 'the current 9.x' because its mirror layout does, and a
        # point release there is just a newer file behind the same name. Here the
        # ISO is a specific file you downloaded, so the version names exactly
        # which one, and 00-subscription.sh pins dnf to the same minor. Asking
        # for 9.7 and silently being upgraded to 9.8 on first boot would defeat
        # the point of choosing a minor release.
        Version = '9.7'

        # Minor releases matter here, so 9 on its own is refused: it does not say
        # which ISO. RHEL 10 is allowed by the pattern but untested with this
        # blueprint, and note that EPEL 10 still has no xrdp, so Enhanced Session
        # would be lost the same way it is on Rocky 10.
        VersionPattern = '^(9|10)\.\d+$'
        SupportPolicy  = 'RHEL 9: full support to May 2027, maintenance to May 2032'
    }

    Media = @{
        # Deliberately no IsoBaseUrl and no ChecksumFile. There is no anonymous
        # URL to resolve and no published checksum to verify against, so the
        # download-and-verify path does not apply. Resolve-DevVMIso reads these
        # two keys to explain that instead of failing with a generic message.
        RequiresManualDownload = $true
        DownloadPage           = 'https://developers.redhat.com/products/rhel/download'
        # Only used to sanity-check the file that Iso.Path names, so a boot ISO
        # is caught before a ninety minute install discovers it has no packages.
        IsoNamePattern         = '*dvd*.iso'
    }

    Provisioning = @{
        Method        = 'kickstart-oemdrv'   # FAT32 disk labelled OEMDRV
        SeedLabel     = 'OEMDRV'
        SeedFileName  = 'ks.cfg'
        SeedTemplate  = 'seed/ks.cfg.tmpl'

        # The shared steps, plus this blueprint's own 00-subscription.sh, which
        # has no counterpart anywhere else: the rebuilds need no registration.
        # It is numbered 00 so it runs before 01-base.sh, which is the first step
        # that wants a repository.
        BootstrapDirs = @('../bootstrap', 'bootstrap')
        BootstrapRoot = '/usr/local/lib/vm-bootstrap'

        SystemDisk = 'sda'

        # Install from the DVD in the drive, with no network and no entitlement.
        #
        # One word is enough, and the AppStream repo deliberately is NOT listed
        # beside it: the DVD's .treeinfo already points at both BaseOS and
        # AppStream, and Anaconda reads it. Adding the repo by hand would mean
        # hard-coding the media mount path, which moved between RHEL 8
        # (/run/install/repo) and RHEL 9 (/run/install/sources/mount-0000-cdrom)
        # and is exactly the kind of detail that breaks on the next release.
        InstallSource = 'cdrom'

        PowerOffAfterInstall  = $true


        # Hyper-V's guest daemons. They stay in Packages so they are there from the

        # first boot on Hyper-V; Vmynt leaves them out on other hypervisors, where

        # they would only wait for a Hyper-V device that never appears.

        HyperVOnlyPackages = @('hyperv-daemons')
        # The DVD install is local, so it is faster than the netinst blueprints,
        # but leave headroom for a slow laptop disk.
        InstallTimeoutMinutes = 90
    }

    Firmware = @{
        Generation          = 2
        # RHEL's shim is signed by the Microsoft UEFI CA, same as Fedora's,
        # Rocky's and Ubuntu's.
        SecureBootTemplate  = 'MicrosoftUEFICertificateAuthority'
        EnableTpm           = $false
    }

    # Everything here is on the DVD, so the install needs no network. EPEL
    # packages wait for bootstrap, which is the first point a repository exists.
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
        # Present on the DVD and needed by the very first bootstrap step, so it
        # is laid down here rather than fetched from a repo that may not exist
        # yet on an unregistered machine.
        'subscription-manager'
    )

    Defaults = @{
        VM = @{
            MemoryStartupGB = 12
            MemoryMaximumGB = 16
        }
        Bootstrap = @{
            # Same comps group as Fedora and Rocky. Normally a no-op, since the
            # kickstart has already installed the environment.
            DesktopPackage = '@workstation-product-environment'
        }
    }

    # ---------------------------------------------------------------- arm64
    # Applied over everything above when the guest is arm64, which is every
    # guest on an Apple Silicon Mac. Tables merge key by key and lists replace
    # outright, so Packages below is the whole list, not an addition. The
    # PowerShell tool only builds x86_64 and ignores this block.
    #
    # Red Hat ships an aarch64 binary DVD behind the same account, named like
    # rhel-9.7-aarch64-dvd.iso, so only the name check changes. Download it
    # yourself and set isoPath, exactly as on x86_64.
    Arm64 = @{
        Media = @{
            IsoNamePattern = '*aarch64*dvd*.iso'
        }

        # As above, less hyperv-daemons: there is no Hyper-V on an arm64 Mac.
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
            'python3'
            'subscription-manager'
        )
    }
}
