extern crate globset;

use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use std::fs;
use std::io;
use std::io::Read;
use std::path::{Path, PathBuf};

pub fn load(path: &Path) -> Option<GitignoreFile> {
    let mut p = path.to_owned();

    loop {
        let gitignore_path = p.join(".gitignore");
        if gitignore_path.exists() {
            return GitignoreFile::new(&gitignore_path).ok();
        }

        // Stop if we see a .git directory
        if let Ok(metadata) = p.join(".git").metadata() {
            if metadata.is_dir() {
                break;
            }
        }

        if p.parent().is_none() {
            break;
        }

        p.pop();
    }

    None

}

pub struct GitignoreFile {
    set: GlobSet,
    patterns: Vec<Pattern>,
    root: PathBuf,
}

#[derive(Debug)]
pub enum Error {
    GlobSet(globset::Error),
    Io(io::Error),
}

struct Pattern {
    pattern: String,
    pattern_type: PatternType,
    anchored: bool,
}

enum PatternType {
    Ignore,
    Whitelist,
}

impl GitignoreFile {
    pub fn new(path: &Path) -> Result<GitignoreFile, Error> {
        let mut file = try!(fs::File::open(path));
        let mut contents = String::new();
        try!(file.read_to_string(&mut contents));

        let lines = contents.lines().collect();
        let root = path.parent().unwrap();

        GitignoreFile::from_strings(lines, root)
    }

    pub fn from_strings(strs: Vec<&str>, root: &Path) -> Result<GitignoreFile, Error> {
        let mut builder = GlobSetBuilder::new();
        let mut patterns = vec![];

        let parsed_patterns = GitignoreFile::parse(strs);
        for p in parsed_patterns {
            let mut pat = String::from(p.pattern.clone());
            if !p.anchored && !pat.starts_with("**/") {
                pat = "**/".to_string() + &pat;
            }

            if !pat.ends_with("/**") {
                pat = pat + "/**";
            }

            let glob = try!(GlobBuilder::new(&pat)
                            .literal_separator(true)
                            .build());

            builder.add(glob);
            patterns.push(p);
        }

        Ok(GitignoreFile {
            set: try!(builder.build()),
            patterns: patterns,
            root: root.to_owned(),
        })

    }

    pub fn is_excluded(&self, path: &Path) -> bool {
        let stripped = path.strip_prefix(&self.root);
        if !stripped.is_ok() {
            return false;
        }

        let matches = self.set.matches(stripped.unwrap());

        for &i in matches.iter().rev() {
            let pattern = &self.patterns[i];
            return match pattern.pattern_type {
                PatternType::Whitelist  => false,
                PatternType::Ignore     => true,
            }
        }

        false
    }

    fn parse(contents: Vec<&str>) -> Vec<Pattern> {
        contents.iter()
            .filter(|l| !l.is_empty())
            .filter(|l| !l.starts_with('#'))
            .map(|l| Pattern::parse(l))
            .collect()
    }
}

impl Pattern {
    fn parse(pattern: &str) -> Pattern {
        let mut normalized = String::from(pattern);

        let pattern_type = if normalized.starts_with('!') {
            normalized.remove(0);
            PatternType::Whitelist
        } else {
            PatternType::Ignore
        };

        let anchored = if normalized.starts_with('/') {
            normalized.remove(0);
            true
        } else {
            false
        };

        if normalized.ends_with('/') {
            normalized.pop();
        }

        if normalized.starts_with("\\#") || normalized.starts_with("\\!") {
            normalized.remove(0);
        }

        Pattern {
            pattern: normalized,
            pattern_type: pattern_type,
            anchored: anchored,
        }
    }
}


impl From<globset::Error> for Error {
    fn from(error: globset::Error) -> Error {
        Error::GlobSet(error)
    }
}

impl From<io::Error> for Error {
    fn from(error: io::Error) -> Error {
        Error::Io(error)
    }
}


#[cfg(test)]
mod tests {
    use super::GitignoreFile;
    use std::path::PathBuf;

    fn base_dir() -> PathBuf {
        PathBuf::from("/home/user/dir")
    }

    fn build_gitignore(pattern: &str) -> GitignoreFile {
        GitignoreFile::from_strings(vec![pattern], &base_dir()).unwrap()
    }

    #[test]
    fn test_matches_exact() {
        let file = build_gitignore("Cargo.toml");

        assert!(file.is_excluded(&base_dir().join("Cargo.toml")));
    }

    #[test]
    fn test_does_not_match() {
        let file = build_gitignore("Cargo.toml");

        assert!(!file.is_excluded(&base_dir().join("src").join("main.rs")));
    }

    #[test]
    fn test_matches_simple_wildcard() {
        let file = build_gitignore("targ*");

        assert!(file.is_excluded(&base_dir().join("target")));
    }

    #[test]
    fn test_matches_subdir_exact() {
        let file = build_gitignore("target");

        assert!(file.is_excluded(&base_dir().join("target/")));
    }

    #[test]
    fn test_matches_subdir() {
        let file = build_gitignore("target");

        assert!(file.is_excluded(&base_dir().join("target").join("file")));
        assert!(file.is_excluded(&base_dir().join("target").join("subdir").join("file")));
    }

    #[test]
    fn test_wildcard_with_dir() {
        let file = build_gitignore("target/f*");

        assert!(file.is_excluded(&base_dir().join("target").join("file")));
        assert!(!file.is_excluded(&base_dir().join("target").join("subdir").join("file")));
    }

    #[test]
    fn test_leading_slash() {
        let file = build_gitignore("/*.c");

        assert!(file.is_excluded(&base_dir().join("cat-file.c")));
        assert!(!file.is_excluded(&base_dir().join("mozilla-sha1").join("sha1.c")));
    }

    #[test]
    fn test_leading_double_wildcard() {
        let file = build_gitignore("**/foo");

        assert!(file.is_excluded(&base_dir().join("foo")));
        assert!(file.is_excluded(&base_dir().join("target").join("foo")));
        assert!(file.is_excluded(&base_dir().join("target").join("subdir").join("foo")));
    }

    #[test]
    fn test_trailing_double_wildcard() {
        let file = build_gitignore("abc/**");

        assert!(!file.is_excluded(&base_dir().join("def").join("foo")));
        assert!(file.is_excluded(&base_dir().join("abc").join("foo")));
        assert!(file.is_excluded(&base_dir().join("abc").join("subdir").join("foo")));
    }

    #[test]
    fn test_sandwiched_double_wildcard() {
        let file = build_gitignore("a/**/b");

        assert!(file.is_excluded(&base_dir().join("a").join("b")));
        assert!(file.is_excluded(&base_dir().join("a").join("x").join("b")));
        assert!(file.is_excluded(&base_dir().join("a").join("x").join("y").join("b")));
    }

    #[test]
    fn test_empty_file_never_excludes() {
        let file = GitignoreFile::from_strings(vec![], &base_dir()).unwrap();

        assert!(!file.is_excluded(&base_dir().join("target")));
    }

    #[test]
    fn test_checks_all_patterns() {
        let patterns = vec!["target", "target2"];
        let file = GitignoreFile::from_strings(patterns, &base_dir()).unwrap();

        assert!(file.is_excluded(&base_dir().join("target").join("foo.txt")));
        assert!(file.is_excluded(&base_dir().join("target2").join("bar.txt")));
    }

    #[test]
    fn test_handles_whitelisting() {
        let patterns = vec!["target", "!target/foo.txt"];
        let file = GitignoreFile::from_strings(patterns, &base_dir()).unwrap();

        assert!(!file.is_excluded(&base_dir().join("target").join("foo.txt")));
        assert!(file.is_excluded(&base_dir().join("target").join("blah.txt")));
    }
}

