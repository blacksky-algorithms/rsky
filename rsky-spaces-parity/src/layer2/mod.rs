//! Support code for the two-process acceptance gate: token minting, a stub DID
//! directory, child-process supervision, and the response normalizers the gate
//! compares with.

/// Every request in the gate is to a loopback port, so proxy discovery is both
/// useless and, on macOS, a hard failure inside a restricted sandbox.
pub fn http_client() -> anyhow::Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .no_proxy()
        .timeout(std::time::Duration::from_secs(30))
        .build()?)
}

pub mod car;
pub mod directory;
pub mod normalize;
pub mod process;
pub mod tokens;

/// One line of the gate's scoreboard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Check {
    pub name: String,
    pub verdict: Verdict,
    pub detail: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Both sides agreed.
    Equal,
    /// The sides disagreed in a way the gate treats as failure.
    Differs,
    /// The sides disagreed in a way this design accepts and records.
    Documented,
    /// Recorded for the report, not scored.
    Note,
}

impl Verdict {
    pub fn tag(self) -> &'static str {
        match self {
            Verdict::Equal => "equal",
            Verdict::Differs => "DIFFERS",
            Verdict::Documented => "documented divergence",
            Verdict::Note => "note",
        }
    }
}

#[derive(Debug, Default)]
pub struct Scoreboard {
    pub checks: Vec<Check>,
}

impl Scoreboard {
    pub fn push(&mut self, name: impl Into<String>, verdict: Verdict, detail: impl Into<String>) {
        self.checks.push(Check {
            name: name.into(),
            verdict,
            detail: detail.into(),
        });
    }

    pub fn equal_if(
        &mut self,
        name: impl Into<String>,
        equal: bool,
        detail: impl Into<String>,
    ) -> bool {
        let verdict = if equal {
            Verdict::Equal
        } else {
            Verdict::Differs
        };
        self.push(name, verdict, detail);
        equal
    }

    pub fn scored(&self) -> usize {
        self.checks
            .iter()
            .filter(|c| matches!(c.verdict, Verdict::Equal | Verdict::Differs))
            .count()
    }

    pub fn passed(&self) -> usize {
        self.checks
            .iter()
            .filter(|c| c.verdict == Verdict::Equal)
            .count()
    }

    pub fn failures(&self) -> usize {
        self.checks
            .iter()
            .filter(|c| c.verdict == Verdict::Differs)
            .count()
    }

    pub fn documented(&self) -> usize {
        self.checks
            .iter()
            .filter(|c| c.verdict == Verdict::Documented)
            .count()
    }

    pub fn render(&self) -> String {
        let mut out = String::new();
        for check in &self.checks {
            out.push_str(&format!("  [{}] {}\n", check.verdict.tag(), check.name));
            if !check.detail.is_empty() {
                for line in check.detail.lines() {
                    out.push_str(&format!("        {line}\n"));
                }
            }
        }
        out.push_str(&format!(
            "\nparity: {}/{} checks equal (+{} documented divergence)\n",
            self.passed(),
            self.scored(),
            self.documented()
        ));
        out
    }
}
