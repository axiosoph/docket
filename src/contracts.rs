//! Docket's own Nickel contracts (`contracts/docket.ncl`,
//! `contracts/register.ncl`, and the `claim.ncl`/`quadrant.ncl` they
//! import), embedded at compile time.
//!
//! These ship WITH the tool, not with a corpus (MVP.md §7 draws that
//! line): `docket.ncl`'s shape contract and the register evaluator are
//! fixed for a given docket build, so a caller can never be expected to
//! have them on disk at any particular path. A path resolved relative to
//! the current directory is only ever correct by coincidence — the same
//! defect `hooks/pre-commit` already had to fix by resolving relative to
//! itself instead of the corpus being checked — and a path resolved
//! relative to the running binary breaks the moment the binary is
//! installed anywhere its build tree's `contracts/` doesn't travel with
//! it (`cargo install`, a copied release binary). `include_str!` bakes
//! the content into the binary itself, so neither failure mode exists:
//! the tool always carries its own contracts, wherever it runs.
//!
//! Nickel's `import` resolves relative to the importing *file's* location
//! on disk, not to any notion of a virtual filesystem, so the embedded
//! content still has to land on disk before Nickel can see it.
//! [`MaterializedContracts::new`] writes all four files into one fresh
//! temp directory, preserving the same flat layout they have in
//! `contracts/`, so `docket.ncl`'s `import "claim.ncl"` and
//! `register.ncl`'s `import "claim.ncl"` resolve unmodified. The
//! directory is removed when the value drops — callers must keep it
//! alive until Nickel has finished reading from it.

use std::io;
use std::path::PathBuf;

const CLAIM_NCL: &str = include_str!("../contracts/claim.ncl");
const QUADRANT_NCL: &str = include_str!("../contracts/quadrant.ncl");
const DOCKET_NCL: &str = include_str!("../contracts/docket.ncl");
const REGISTER_NCL: &str = include_str!("../contracts/register.ncl");

/// A temp directory holding docket's own contracts. Removed on drop, so
/// a caller must hold the value for as long as any path it returns needs
/// to resolve on disk.
pub struct MaterializedContracts {
    dir: PathBuf,
}

impl MaterializedContracts {
    pub fn new() -> io::Result<Self> {
        let dir = std::env::temp_dir().join(format!(
            "docket-contracts-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or_default(),
        ));
        std::fs::create_dir_all(&dir)?;
        for (name, content) in [
            ("claim.ncl", CLAIM_NCL),
            ("quadrant.ncl", QUADRANT_NCL),
            ("docket.ncl", DOCKET_NCL),
            ("register.ncl", REGISTER_NCL),
        ] {
            std::fs::write(dir.join(name), content)?;
        }
        Ok(Self { dir })
    }

    /// The materialized `docket.ncl` — the config shape contract.
    pub fn docket_path(&self) -> PathBuf {
        self.dir.join("docket.ncl")
    }

    /// The materialized `register.ncl` — the register evaluator.
    pub fn register_path(&self) -> PathBuf {
        self.dir.join("register.ncl")
    }
}

impl Drop for MaterializedContracts {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn materializes_all_four_files_with_resolvable_imports() {
        let m = MaterializedContracts::new().expect("temp dir is writable");
        assert!(m.docket_path().is_file());
        assert!(m.register_path().is_file());
        assert!(m.dir.join("claim.ncl").is_file());
        assert!(m.dir.join("quadrant.ncl").is_file());

        // `typecheck` rather than `export`: both files are contracts/
        // functions, not data values, so exporting them as JSON fails on
        // shape even when imports resolve fine. Typechecking still has
        // to resolve every `import` to check the record it defines, so
        // it exercises exactly the thing this test is for — and a
        // negative control (typechecking docket.ncl from a directory
        // missing claim.ncl) confirms it actually fails when imports
        // don't resolve, rather than passing vacuously.
        for contract in [m.docket_path(), m.register_path()] {
            let output = std::process::Command::new("nickel")
                .arg("typecheck")
                .arg(&contract)
                .output()
                .expect("nickel is on PATH for tests");
            assert!(
                output.status.success(),
                "{} failed to typecheck from the materialized dir: {}",
                contract.display(),
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }

    #[test]
    fn each_instance_gets_its_own_directory_cleaned_up_on_drop() {
        let a = MaterializedContracts::new().unwrap();
        let b = MaterializedContracts::new().unwrap();
        assert_ne!(a.dir, b.dir);
        let a_dir = a.dir.clone();
        drop(a);
        assert!(!a_dir.exists());
    }
}
