//Vmynt
//Licensed under the MIT License. See the LICENSE file in the project root.
//Copyright (c) 2026 Scarlett A. Scott (codescarlett)
//
//Contributors:
//Scarlett A. Scott (codescarlett)

//Install media: find the ISO a blueprint describes, download it into the cache
//(~/.vmynt/isos), and check it against the vendor's published SHA-256. A
//blueprint describes its media one of three ways:
//
//  1. iso.path in config               a local .iso, used as it is
//  2. Media.IsoUrl + Media.IsoSha256   a pinned download
//  3. Media.IsoBaseUrl + ChecksumFile + IsoNamePattern
//                                      the current point release, read from the
//                                      vendor's checksum file, so the blueprint
//                                      never goes stale

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};

use crate::config::effective::Effective;
use crate::utilities::ui::Ui;

//Resolves, downloads if needed, and verifies. Returns the ISO's path.
pub fn resolve(config: &Effective, ui: Ui) -> Result<PathBuf> {
    let blueprint = &config.blueprint;
    let verify = config.flag("Iso.VerifyChecksum", true);

    if let Some(explicit) = config.string("Iso.Path") {
        let path = config.config_path(&explicit);
        if !path.is_file() {
            bail!(
                "iso.path points at a file that does not exist: {}",
                path.display()
            );
        }
        ui.detail(&format!(
            "using the ISO named in config: {}",
            path.display()
        ));
        if blueprint.get_bool("Media.RequiresManualDownload", false)
            && let Some(pattern) = blueprint.get_string("Media.IsoNamePattern")
        {
            let leaf = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            if !wildcard_match(&pattern, &leaf) {
                ui.warn(&format!(
                    "the ISO is named '{leaf}', which does not match the expected '{pattern}' for {}. This blueprint \
                     installs from the full DVD; a boot ISO has no packages and will not finish unattended.",
                    blueprint.name
                ));
            }
        }
        return Ok(path);
    }

    if blueprint.get_bool("Media.RequiresManualDownload", false) {
        let page = blueprint
            .get_string("Media.DownloadPage")
            .map(|p| format!("\n  Download it from: {p}"))
            .unwrap_or_default();
        bail!(
            "blueprint '{}' cannot download its own install media: the vendor requires an account.{page}\n  \
             Download the DVD yourself, then point config at it:\n      isoPath: ~/Downloads/rhel-9.7-{}-dvd.iso\n  \
             redhat-rocky-workstation is binary-compatible with RHEL and downloads its own media.",
            blueprint.name,
            config.arch.as_str()
        );
    }

    let directory = PathBuf::from(config.string_or("Iso.Directory", "."));
    std::fs::create_dir_all(&directory)
        .with_context(|| format!("could not create {}", directory.display()))?;

    let (url, sha) = if let Some(url) = blueprint.get_string("Media.IsoUrl") {
        (url, blueprint.get_string("Media.IsoSha256"))
    } else if let Some(base) = blueprint.get_string("Media.IsoBaseUrl") {
        let release = config.string_or("Iso.Release", "");
        let mut base = base.replace("{Release}", &release);
        if !base.ends_with('/') {
            base.push('/');
        }
        let checksum_pattern = blueprint
            .get_string("Media.ChecksumFile")
            .unwrap_or_else(|| "SHA256SUMS".to_string());
        let name_pattern = blueprint
            .get_string("Media.IsoNamePattern")
            .unwrap_or_else(|| "*.iso".to_string());
        let checksum_file = if checksum_pattern.contains(['*', '?']) {
            ui.detail(&format!(
                "looking for '{checksum_pattern}' in the release folder"
            ));
            let listing = get_text(&base)?;
            pick_from_listing(&listing, &checksum_pattern).with_context(|| {
                format!(
                    "no file matching '{checksum_pattern}' is listed at {base}. Check osVersion, or pin Media.IsoUrl and Media.IsoSha256."
                )
            })?
        } else {
            checksum_pattern
        };
        ui.detail(&format!(
            "resolving the current release from {base}{checksum_file}"
        ));
        let sums = get_text(&format!("{base}{checksum_file}"))?;
        let (name, hash) = select_checksum_entry(&sums, &name_pattern).with_context(|| {
            format!(
                "no entry matching '{name_pattern}' in the vendor's checksum file. Check osVersion and the blueprint's Media.IsoNamePattern."
            )
        })?;
        ui.detail(&format!("release resolved: {name}"));
        (format!("{base}{name}"), Some(hash))
    } else {
        bail!(
            "blueprint '{}' does not describe any install media. Set Media.IsoUrl or Media.IsoBaseUrl, or point iso.path at a local file.",
            blueprint.name
        );
    };

    let file_name = url
        .rsplit('/')
        .next()
        .filter(|name| !name.is_empty())
        .context("the ISO URL has no file name")?;
    let target = directory.join(file_name);

    if target.is_file() {
        match (&sha, verify) {
            (Some(hash), true) => {
                ui.detail("verifying the cached ISO");
                if sha256_file(&target)?.eq_ignore_ascii_case(hash) {
                    ui.detail(&format!("cached ISO verified: {}", target.display()));
                    return Ok(target);
                }
                ui.warn("the cached ISO failed checksum verification; downloading it again");
                std::fs::remove_file(&target).ok();
            }
            _ => {
                ui.detail(&format!(
                    "reusing {} (verification off or no published hash)",
                    target.display()
                ));
                return Ok(target);
            }
        }
    }

    ui.detail(&format!("downloading {url}"));
    let actual = download(&url, &target, ui)?;
    match (&sha, verify) {
        (Some(hash), true) => {
            if !actual.eq_ignore_ascii_case(hash) {
                std::fs::remove_file(&target).ok();
                bail!(
                    "the downloaded ISO failed SHA-256 verification and has been deleted. Run the command again to retry."
                );
            }
            ui.detail("checksum ok");
        }
        (None, true) => {
            ui.warn("no published checksum was available, so the download could not be verified")
        }
        _ => {}
    }
    Ok(target)
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(20)))
        .user_agent(format!("{}/{}", crate::brand::NAME, crate::brand::VERSION))
        .build()
        .into()
}

fn get_text(url: &str) -> Result<String> {
    let mut response = agent()
        .get(url)
        .call()
        .with_context(|| format!("could not fetch {url}"))?;
    let text = response
        .body_mut()
        .with_config()
        .limit(16 << 20)
        .read_to_string()
        .with_context(|| format!("could not read {url}"))?;
    Ok(text.trim_start_matches('\u{feff}').to_string())
}

//Streams a download to `<target>.part`, hashing as it goes, then renames it.
//Returns the SHA-256.
fn download(url: &str, target: &Path, ui: Ui) -> Result<String> {
    let partial = PathBuf::from(format!("{}.part", target.display()));
    let response = agent()
        .get(url)
        .call()
        .with_context(|| format!("could not download {url}"))?;
    let total: Option<u64> = response
        .headers()
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse().ok());
    let mut reader = response.into_body().into_reader();
    let mut file = std::fs::File::create(&partial)
        .with_context(|| format!("could not create {}", partial.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1 << 20];
    let mut done: u64 = 0;
    let mut last = Instant::now();
    let started = Instant::now();
    loop {
        let read = reader
            .read(&mut buffer)
            .context("the download was interrupted")?;
        if read == 0 {
            break;
        }
        file.write_all(&buffer[..read])?;
        hasher.update(&buffer[..read]);
        done += read as u64;
        if last.elapsed() > Duration::from_millis(500) {
            last = Instant::now();
            ui.progress(&progress_line(done, total, started.elapsed()));
        }
    }
    ui.progress_done();
    file.sync_all()?;
    drop(file);
    std::fs::rename(&partial, target)
        .with_context(|| format!("could not move the download to {}", target.display()))?;
    Ok(hex(&hasher.finalize()))
}

fn progress_line(done: u64, total: Option<u64>, elapsed: Duration) -> String {
    let mb = |bytes: u64| bytes as f64 / (1 << 20) as f64;
    let rate = mb(done) / elapsed.as_secs_f64().max(0.001);
    match total {
        Some(total) if total > 0 => format!(
            "downloading {:.0} of {:.0} MB ({:.0}%), {rate:.1} MB/s",
            mb(done),
            mb(total),
            100.0 * done as f64 / total as f64
        ),
        _ => format!("downloading {:.0} MB, {rate:.1} MB/s", mb(done)),
    }
}

pub fn sha256_file(path: &Path) -> Result<String> {
    let mut file =
        std::fs::File::open(path).with_context(|| format!("could not open {}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1 << 20];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex(&hasher.finalize()))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

//Shell-style matching: * is any run of characters, ? is one. Case-insensitive.
pub fn wildcard_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.to_lowercase().chars().collect();
    let t: Vec<char> = text.to_lowercase().chars().collect();
    let (mut pi, mut ti) = (0, 0);
    let (mut star, mut mark) = (None, 0);
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = ti;
            pi += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

//The entry for `pattern` in a checksum file. Both common formats:
//  <hash> *name.iso                      (sha256sum)
//  SHA256 (name.iso) = <hash>            (BSD, Fedora and Rocky)
//The highest name wins when several match.
pub fn select_checksum_entry(content: &str, pattern: &str) -> Option<(String, String)> {
    let gnu = regex_lite::Regex::new(r"^([0-9a-fA-F]{64})\s+\*?(.+)$").expect("valid");
    let bsd = regex_lite::Regex::new(r"^SHA256\s*\(\s*(.+?)\s*\)\s*=\s*([0-9a-fA-F]{64})$")
        .expect("valid");
    let mut found: Vec<(String, String)> = Vec::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let entry = if let Some(caps) = gnu.captures(line) {
            (caps[2].trim().to_string(), caps[1].to_lowercase())
        } else if let Some(caps) = bsd.captures(line) {
            (caps[1].trim().to_string(), caps[2].to_lowercase())
        } else {
            continue;
        };
        if wildcard_match(pattern, &entry.0) {
            found.push(entry);
        }
    }
    found.sort();
    found.pop()
}

//The highest file name in a web server's folder listing matching `pattern`.
pub fn pick_from_listing(html: &str, pattern: &str) -> Option<String> {
    let href = regex_lite::Regex::new(r#"href\s*=\s*["']([^"']+)["']"#).expect("valid");
    let mut names: Vec<String> = href
        .captures_iter(html)
        .filter_map(|caps| caps[1].rsplit(['/', '\\']).next().map(str::to_string))
        .filter(|leaf| !leaf.is_empty() && wildcard_match(pattern, leaf))
        .collect();
    names.sort();
    names.dedup();
    names.pop()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wildcards() {
        assert!(wildcard_match("*-boot.iso", "Rocky-9.6-x86_64-boot.iso"));
        assert!(wildcard_match(
            "Rocky-*-latest-x86_64-boot.iso.CHECKSUM",
            "rocky-9-latest-x86_64-boot.iso.checksum"
        ));
        assert!(wildcard_match(
            "*live-server-amd64.iso",
            "ubuntu-24.04.3-live-server-amd64.iso"
        ));
        assert!(!wildcard_match(
            "*live-server-amd64.iso",
            "ubuntu-24.04.3-desktop-amd64.iso"
        ));
        assert!(wildcard_match("a?c", "abc"));
    }

    #[test]
    fn checksum_files_in_both_formats() {
        let gnu = "aaaa0000aaaa0000aaaa0000aaaa0000aaaa0000aaaa0000aaaa0000aaaa0000 *ubuntu-24.04.2-live-server-amd64.iso\n\
                   bbbb0000bbbb0000bbbb0000bbbb0000bbbb0000bbbb0000bbbb0000bbbb0000 *ubuntu-24.04.3-live-server-amd64.iso\n";
        let (name, hash) = select_checksum_entry(gnu, "*live-server-amd64.iso").unwrap();
        assert_eq!(name, "ubuntu-24.04.3-live-server-amd64.iso");
        assert!(hash.starts_with("bbbb"));
        let bsd = "# Rocky-9-latest-x86_64-boot.iso: 1 bytes\nSHA256 (Rocky-9-latest-x86_64-boot.iso) = CCCC0000CCCC0000CCCC0000CCCC0000CCCC0000CCCC0000CCCC0000CCCC0000\n";
        let (name, hash) = select_checksum_entry(bsd, "*-boot.iso").unwrap();
        assert_eq!(name, "Rocky-9-latest-x86_64-boot.iso");
        assert!(hash.starts_with("cccc"));
        assert!(select_checksum_entry(bsd, "*dvd*.iso").is_none());
    }

    #[test]
    fn listings_are_scraped() {
        let html =
            r#"<a href="Fedora-Everything-43-1.6-x86_64-CHECKSUM">x</a> <a href='other.txt'>y</a>"#;
        assert_eq!(
            pick_from_listing(html, "*-CHECKSUM").as_deref(),
            Some("Fedora-Everything-43-1.6-x86_64-CHECKSUM")
        );
    }
}
