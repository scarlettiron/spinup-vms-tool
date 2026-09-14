<#
    Blueprint: ubuntu-server

    Identical to ubuntu-desktop but headless. Also the cheapest possible example
    of extending this repo: inherit another blueprint's media, templates, and
    bootstrap steps, then state the difference.

    Defaults vs Overrides is the distinction that matters:

      Defaults  - what this OS *suggests*. Sits BELOW vm.config.yaml, so the
                  team standard can override it. Sizing belongs here.
      Overrides - what this OS *is*. Sits ABOVE vm.config.yaml, so choosing this
                  blueprint actually produces a headless machine instead of being
                  quietly undone by an InstallDesktop = $true in the shared
                  config. A developer's own vm.config.local.yaml, a Machines
                  block, or a command-line parameter still wins.
#>
@{
    Name        = 'ubuntu-server'
    Description = 'Ubuntu LTS, headless, same dev toolchain'
    Inherits    = 'ubuntu-desktop'

    Overrides = @{
        VM = @{
            # Enhanced Session is an RDP-into-GNOME feature; there is no GNOME.
            EnhancedSession = $false
        }
        Bootstrap = @{
            InstallDesktop = $false
            InstallVSCode  = $false
            # 41-chrome.sh skips itself without a desktop anyway, but saying so
            # here keeps the plan honest rather than listing chrome and not
            # installing it.
            InstallChrome  = $false
        }
    }

    Defaults = @{
        VM = @{
            CpuCount        = 4
            MemoryStartupGB = 8
            MemoryMinimumGB = 2
            MemoryMaximumGB = 12
            DiskSizeGB      = 80
        }
        Bootstrap = @{
            # Nothing to reboot into on a headless box.
            RebootWhenDone = $false
        }
    }
}
