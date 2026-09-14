<#
    Blueprint: ubuntu-desktop

    Installs Ubuntu Server unattended from the official live-server ISO, then
    layers the full GNOME desktop on during first boot.

    Why not the desktop ISO? The server ISO's autoinstall path is the one
    Canonical exercises hardest, it needs no ISO repacking, and installing the
    'ubuntu-desktop' meta-package afterwards yields the same desktop while
    keeping the package set explicit and diffable. See docs/adding-an-os.md.
#>
@{
    Name        = 'ubuntu-desktop'
    Description = 'Ubuntu LTS + GNOME desktop, provisioned as a dev workstation'

    Os = @{
        Family  = 'linux'
        Vendor  = 'ubuntu'

        # The release this blueprint installs, used when OsVersion is left blank
        # in config.
        #
        # 24.04 is an LTS: five years of standard support, to April 2029. Ubuntu
        # LTS releases land every April of an even year, so 26.04 LTS is the newer
        # one. This defaults to 24.04 because it is the LTS that is certainly
        # released and past its first point release; moving to 26.04 is a
        # one-word change once you are happy to.
        Version        = '24.04'

        # Ubuntu releases are YY.MM. Checked at build time so pointing this
        # blueprint at '42' fails immediately.
        VersionPattern = '^\d{2}\.\d{2}$'
        SupportPolicy  = 'LTS: 5 years standard support; interim: 9 months'
    }

    Media = @{
        # SHA256SUMS in this directory is parsed to resolve the exact point
        # release and verify the download. {Release} comes from Iso.Release.
        IsoBaseUrl     = 'https://releases.ubuntu.com/{Release}/'
        IsoNamePattern = '*live-server-amd64.iso'
        ChecksumFile   = 'SHA256SUMS'
    }

    # How unattended-install config reaches the guest.
    Provisioning = @{
        Method        = 'cloud-init-nocloud'  # FAT32 VHDX labelled CIDATA
        SeedTemplate  = 'seed/user-data.tmpl'
        MetaTemplate  = 'seed/meta-data.tmpl'
        SeedLabel     = 'CIDATA'
        SeedFileName  = 'user-data'
        # The bootstrap steps come from blueprints/bootstrap; this blueprint
        # adds only the ones whose package names or mechanisms differ. Later
        # directories win, so a file here replaces the shared one.
        BootstrapDirs = @('../bootstrap', 'bootstrap')
        # Not /opt: that mount point is available for a data disk, and
        # remounting it out from under the running bootstrap tree would be a
        # spectacular way to fail halfway through provisioning.
        BootstrapRoot = '/usr/local/lib/vm-bootstrap'
        # autoinstall 'source.id' - ubuntu-server(-minimal) on the server ISO
        SourceId      = 'ubuntu-server'
        # The installer powers off when done so the host can eject the ISO
        # deterministically instead of racing the post-install reboot.
        PowerOffAfterInstall = $true
        InstallTimeoutMinutes = 60
    }

    Firmware = @{
        Generation          = 2
        SecureBootTemplate  = 'MicrosoftUEFICertificateAuthority'
        EnableTpm           = $false
    }

    # Packages the installer itself lays down; everything else is bootstrap's job.
    Packages = @(
        'openssh-server'
        'curl'
        'wget'
        'ca-certificates'
        'gnupg'
        'git'
        'jq'
        'unzip'
        'linux-tools-virtual'
        'linux-cloud-tools-virtual'
    )

    Defaults = @{
        Bootstrap = @{
            # The desktop meta-package, named the way THIS OS names it. It lives
            # here rather than in vm.config.yaml because the value is meaningless
            # to any other blueprint: dnf has never heard of 'ubuntu-desktop'.
            DesktopPackage = 'ubuntu-desktop'
        }
    }
}
