//Vmynt
//Licensed under the MIT License. See the LICENSE file in the project root.
//Copyright (c) 2026 Scarlett A. Scott (codescarlett)
//
//Contributors:
//Scarlett A. Scott (codescarlett)

//Password hashing. Linux stores passwords as SHA-512 crypt hashes, the strings
//that start with "$6$". Making one here means the config never needs the plain
//password, and nobody needs openssl installed (the PowerShell tool did).

use anyhow::{Result, anyhow};
#[cfg(test)]
use sha_crypt::PasswordVerifier;
use sha_crypt::{PasswordHasher, ShaCrypt};

//The prefix every SHA-512 crypt hash starts with.
pub const SHA512_PREFIX: &str = "$6$";

//Hashes a password with a fresh random salt.
pub fn hash_password(password: &str) -> Result<String> {
    let hash = ShaCrypt::SHA512
        .hash_password(password.as_bytes())
        .map_err(|error| anyhow!("could not hash the password: {error}"))?;
    Ok(to_glibc_form(hash.as_ref()))
}

//The crate writes a 22-character salt and `rounds=5000`, but SHA-512 crypt
//only ever uses the first 16 salt characters (the crate hashes with those
//too). glibc, which checks passwords on the guest, prints the salt it used and
//compares strings, so a 22-character salt never matches and the password is
//refused. This writes the hash the way glibc and `openssl passwd -6` do:
//$6$<16-character salt>$<hash>.
pub fn to_glibc_form(hash: &str) -> String {
    let Some(rest) = hash.strip_prefix(SHA512_PREFIX) else {
        return hash.to_string();
    };
    let rest = rest.strip_prefix("rounds=5000$").unwrap_or(rest);
    let (prefix, rest) = match rest.strip_prefix("rounds=") {
        Some(after) => match after.split_once('$') {
            Some((rounds, tail)) => (format!("{SHA512_PREFIX}rounds={rounds}$"), tail),
            None => return hash.to_string(),
        },
        None => (SHA512_PREFIX.to_string(), rest),
    };
    match rest.split_once('$') {
        Some((salt, digest)) => {
            let salt: String = salt.chars().take(16).collect();
            format!("{prefix}{salt}${digest}")
        }
        None => hash.to_string(),
    }
}

//True when the password matches the hash. Only the tests need it so far;
//`create` will use it to check a hash before building.
#[cfg(test)]
pub fn verify_password(password: &str, hash: &str) -> bool {
    ShaCrypt::SHA512
        .verify_password(password.as_bytes(), hash)
        .is_ok()
}

//A quick shape check for hashes people paste in.
pub fn looks_like_sha512_hash(hash: &str) -> bool {
    hash.starts_with(SHA512_PREFIX) && hash.matches('$').count() >= 3
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_verify_and_are_salted() {
        let first = hash_password("correct horse").unwrap();
        let second = hash_password("correct horse").unwrap();
        assert!(first.starts_with(SHA512_PREFIX));
        assert_ne!(first, second, "each hash gets its own salt");
        assert!(verify_password("correct horse", &first));
        assert!(!verify_password("wrong", &first));
    }

    #[test]
    fn hashes_have_the_shape_glibc_writes() {
        let hash = hash_password("devpass").unwrap();
        let parts: Vec<&str> = hash.split('$').collect();
        assert_eq!(parts.len(), 4, "{hash}");
        assert_eq!(parts[1], "6");
        assert_eq!(parts[2].len(), 16, "glibc uses 16 salt characters: {hash}");
        assert_eq!(parts[3].len(), 86);
    }

    #[test]
    fn long_salts_are_cut_the_way_glibc_cuts_them() {
        //Seen in a real build: the guest refused this password.
        let crate_form = "$6$rounds=5000$WuNXYLTdWOl.dkKeHzWQP1$FMemow9JLF5F7e1mKp6oCchNqfSRjTiId9KkS4vQUy5qNsAFiRx6jL5AHJKdItU2j9y9j7HhN51euPq02GzK./";
        //What glibc's crypt("devpass", ...) prints for it.
        let glibc = "$6$WuNXYLTdWOl.dkKe$FMemow9JLF5F7e1mKp6oCchNqfSRjTiId9KkS4vQUy5qNsAFiRx6jL5AHJKdItU2j9y9j7HhN51euPq02GzK./";
        assert_eq!(to_glibc_form(crate_form), glibc);
        assert!(verify_password("devpass", glibc));
        assert_eq!(
            to_glibc_form("$6$rounds=10000$abc$def"),
            "$6$rounds=10000$abc$def"
        );
    }

    #[test]
    fn verifies_a_hash_made_by_glibc() {
        //Made with: openssl passwd -6 -salt saltsalt password
        let known = "$6$saltsalt$qFmFH.bQmmtXzyBY0s9v7Oicd2z4XSIecDzlB5KiA2/jctKu9YterLp8wwnSq.qc.eoxqOmSuNp2xS0ktL3nh/";
        assert!(verify_password("password", known));
    }

    #[test]
    fn shape_check() {
        assert!(looks_like_sha512_hash("$6$abc$def"));
        assert!(!looks_like_sha512_hash("$1$abc$def"));
        assert!(!looks_like_sha512_hash("password"));
    }
}
