//! The practice repository of the tutorial: a small VHDL project with a
//! "server" (a bare repository next to it) and a colleague who already sent
//! work, so every lesson has something real to act on. It lives in the
//! system's temporary folder: nothing done in it can touch real work.

use std::path::{Path, PathBuf};

use super::runner::{CommandRecord, GitCommand, run};

/// The folder holding the practice repository, its server and the
/// colleague's copy.
pub fn base_dir() -> PathBuf {
    std::env::temp_dir().join("simple-git-practice")
}

/// The name of the practice repository's folder.
pub const REPO: &str = "led-counter";
const SERVER: &str = "server.git";
const COLLEAGUE: &str = "lea";
const COLLEAGUE_NAME: &str = "Léa";
const COLLEAGUE_EMAIL: &str = "lea@example.com";

/// The commit messages and names the lessons refer to, in the user's
/// language.
pub struct Texts {
    pub first: String,
    pub uart: String,
    pub baud_fast: String,
    pub debounce: String,
    pub readme: String,
    pub trial: String,
    /// The identity used when git has none configured.
    pub fallback_name: String,
}

/// The branch whose merge into `main` conflicts.
pub const CONFLICT_BRANCH: &str = "rx-debounce";
/// A local branch with one commit, to copy or move by drag and drop.
pub const TRIAL_BRANCH: &str = "trial/blink";

const COUNTER: &str = "library ieee;
use ieee.std_logic_1164.all;
use ieee.numeric_std.all;

entity counter is
  generic (WIDTH : natural := 8);
  port (
    clk   : in  std_logic;
    rst   : in  std_logic;
    leds  : out std_logic_vector(WIDTH - 1 downto 0)
  );
end entity;

architecture rtl of counter is
  signal count : unsigned(WIDTH - 1 downto 0) := (others => '0');
begin
  process (clk)
  begin
    if rising_edge(clk) then
      if rst = '1' then
        count <= (others => '0');
      else
        count <= count + 1;
      end if;
    end if;
  end process;
  leds <= std_logic_vector(count);
end architecture;
";

fn uart(baud: &str, extra: &str) -> String {
    format!(
        "library ieee;
use ieee.std_logic_1164.all;

entity uart_tx is
  port (
    clk  : in  std_logic;
    data : in  std_logic_vector(7 downto 0);
    send : in  std_logic;
    tx   : out std_logic
  );
end entity;

architecture rtl of uart_tx is
  constant BAUD : integer := {baud};
{extra}begin
  tx <= '1';
end architecture;
"
    )
}

const README: &str = "# LED counter

A counter shown on the board's LEDs, and a UART transmitter.
";

/// Creates a fresh practice repository in `base` (removing an earlier one)
/// and returns its folder.
pub async fn create(base: &Path, texts: &Texts) -> (Result<PathBuf, String>, Vec<CommandRecord>) {
    let mut records = Vec::new();
    let result = build(base, texts, &mut records).await;
    (result, records)
}

async fn build(
    base: &Path,
    texts: &Texts,
    records: &mut Vec<CommandRecord>,
) -> Result<PathBuf, String> {
    if base.exists() {
        std::fs::remove_dir_all(base).map_err(|e| format!("{}: {e}", base.display()))?;
    }
    std::fs::create_dir_all(base).map_err(|e| format!("{}: {e}", base.display()))?;
    // `/tmp` is a link on macOS: use the real path, as git reports it.
    let base = std::fs::canonicalize(base).unwrap_or_else(|_| base.to_path_buf());
    let server = base.join(SERVER);
    let lea = base.join(COLLEAGUE);
    let repo = base.join(REPO);
    let write = |dir: &Path, file: &str, content: &str| -> Result<(), String> {
        let path = dir.join(file);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::write(&path, content).map_err(|e| format!("{}: {e}", path.display()))
    };

    let server_arg = server.to_string_lossy().into_owned();
    let lea_arg = lea.to_string_lossy().into_owned();
    let repo_arg = repo.to_string_lossy().into_owned();
    git(
        records,
        &base,
        &["init", "--bare", "--initial-branch=main", &server_arg],
    )
    .await?;
    git(records, &base, &["clone", &server_arg, &lea_arg]).await?;
    for (key, value) in [
        ("user.name", COLLEAGUE_NAME),
        ("user.email", COLLEAGUE_EMAIL),
        ("core.autocrlf", "false"),
    ] {
        git(records, &lea, &["config", key, value]).await?;
    }
    // A clone of an empty repository may not be on `main` yet.
    git(records, &lea, &["checkout", "-q", "-B", "main"]).await?;

    write(&lea, "README.md", README)?;
    write(&lea, "src/counter.vhd", COUNTER)?;
    git(records, &lea, &["add", "-A"]).await?;
    commit(records, &lea, &["commit", "-q", "-m", &texts.first], 0).await?;
    write(&lea, "src/uart_tx.vhd", &uart("115200", ""))?;
    git(records, &lea, &["add", "-A"]).await?;
    commit(records, &lea, &["commit", "-q", "-m", &texts.uart], 1).await?;
    git(records, &lea, &["push", "-q", "-u", "origin", "main"]).await?;

    // A branch that changes the same line as `main` will.
    git(records, &lea, &["switch", "-q", "-c", CONFLICT_BRANCH]).await?;
    write(
        &lea,
        "src/uart_tx.vhd",
        &uart(
            "9600",
            "  -- Debounce: a bit must stay stable for 3 clock cycles.\n  constant STABLE : integer := 3;\n",
        ),
    )?;
    commit(records, &lea, &["commit", "-q", "-am", &texts.debounce], 2).await?;
    git(
        records,
        &lea,
        &["push", "-q", "-u", "origin", CONFLICT_BRANCH],
    )
    .await?;
    git(records, &lea, &["switch", "-q", "main"]).await?;
    write(&lea, "src/uart_tx.vhd", &uart("460800", ""))?;
    commit(records, &lea, &["commit", "-q", "-am", &texts.baud_fast], 3).await?;
    git(records, &lea, &["push", "-q"]).await?;

    // The user's copy.
    git(records, &base, &["clone", &server_arg, &repo_arg]).await?;
    git(records, &repo, &["config", "core.autocrlf", "false"]).await?;
    let name = run(GitCommand::new(["config", "--get", "user.name"])
        .in_dir(&repo)
        .background())
    .await;
    let email = run(GitCommand::new(["config", "--get", "user.email"])
        .in_dir(&repo)
        .background())
    .await;
    let missing_name = name.stdout.trim().is_empty();
    let missing_email = email.stdout.trim().is_empty();
    records.extend([name, email]);
    if missing_name {
        git(
            records,
            &repo,
            &["config", "user.name", &texts.fallback_name],
        )
        .await?;
    }
    if missing_email {
        git(records, &repo, &["config", "user.email", "me@example.com"]).await?;
    }
    git(
        records,
        &repo,
        &[
            "branch",
            CONFLICT_BRANCH,
            &format!("origin/{CONFLICT_BRANCH}"),
        ],
    )
    .await?;
    // A trial branch with one commit of its own.
    git(records, &repo, &["switch", "-q", "-c", TRIAL_BRANCH]).await?;
    write(
        &repo,
        "src/blink.vhd",
        "-- Blinks the first LED about once a second.\n",
    )?;
    git(records, &repo, &["add", "-A"]).await?;
    commit(
        records,
        &repo,
        &[
            "-c",
            &format!("user.name={COLLEAGUE_NAME}"),
            "-c",
            &format!("user.email={COLLEAGUE_EMAIL}"),
            "commit",
            "-q",
            "-m",
            &texts.trial,
        ],
        4,
    )
    .await?;
    git(records, &repo, &["switch", "-q", "main"]).await?;

    // Léa sends something new meanwhile: "Get" brings it.
    write(
        &lea,
        "README.md",
        &format!("{README}\n## Pins\n\n- clk: pin E3 (100 MHz)\n- tx: pin D10\n"),
    )?;
    commit(records, &lea, &["commit", "-q", "-am", &texts.readme], 5).await?;
    git(records, &lea, &["push", "-q"]).await?;

    // A change waiting in the user's copy: the counter gets wider.
    write(
        &repo,
        "src/counter.vhd",
        &COUNTER.replace("WIDTH : natural := 8", "WIDTH : natural := 16"),
    )?;
    Ok(repo)
}

async fn git(records: &mut Vec<CommandRecord>, dir: &Path, args: &[&str]) -> Result<(), String> {
    finish(records, GitCommand::new(args.iter().copied()).in_dir(dir)).await
}

/// Runs the `n`th commit of the story, dated `n` hours after the first
/// (the last one an hour ago): with distinct dates, the history always
/// shows the commits in the same order.
async fn commit(
    records: &mut Vec<CommandRecord>,
    dir: &Path,
    args: &[&str],
    n: u64,
) -> Result<(), String> {
    const LAST: u64 = 5;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default();
    let date = format!("@{} +0000", now - (LAST + 1 - n) * 3600);
    finish(
        records,
        GitCommand::new(args.iter().copied())
            .in_dir(dir)
            .with_env("GIT_AUTHOR_DATE", date.clone())
            .with_env("GIT_COMMITTER_DATE", date),
    )
    .await
}

async fn finish(records: &mut Vec<CommandRecord>, command: GitCommand) -> Result<(), String> {
    let record = run(command.background()).await;
    let result = if record.success() {
        Ok(())
    } else {
        Err(record.error_summary())
    };
    records.push(record);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts() -> Texts {
        Texts {
            first: "first".into(),
            uart: "uart".into(),
            baud_fast: "fast".into(),
            debounce: "debounce".into(),
            readme: "readme".into(),
            trial: "trial".into(),
            fallback_name: "Me".into(),
        }
    }

    async fn out(dir: &Path, args: &[&str]) -> String {
        run(GitCommand::new(args.iter().copied()).in_dir(dir))
            .await
            .stdout
    }

    #[tokio::test]
    async fn builds_a_repository_for_every_lesson() {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().join("practice");
        let (repo, records) = create(&base, &texts()).await;
        let repo = repo.unwrap_or_else(|e| panic!("{e}: {records:#?}"));
        // A change to look at, stage and commit.
        assert!(
            out(&repo, &["status", "--porcelain"])
                .await
                .contains("src/counter.vhd")
        );
        // Léa's commit is on the server, not here yet.
        assert!(
            run(GitCommand::new(["fetch", "-q"]).in_dir(&repo))
                .await
                .success()
        );
        assert_eq!(
            out(&repo, &["rev-list", "--count", "main..origin/main"])
                .await
                .trim(),
            "1"
        );
        // The branches the lessons use.
        let branches = out(&repo, &["branch", "--format=%(refname:short)"]).await;
        assert!(branches.contains(CONFLICT_BRANCH));
        assert!(branches.contains(TRIAL_BRANCH));
        // Merging the debounce branch conflicts.
        let merge = run(GitCommand::new(["merge", CONFLICT_BRANCH]).in_dir(&repo)).await;
        assert!(!merge.success());
        assert!(
            out(&repo, &["diff", "--name-only", "--diff-filter=U"])
                .await
                .contains("uart_tx.vhd")
        );
        // Creating it again starts over.
        let (again, _) = create(&base, &texts()).await;
        assert!(again.is_ok());
        assert!(
            out(&again.unwrap(), &["status", "--porcelain"])
                .await
                .lines()
                .all(|l| !l.starts_with("UU"))
        );
    }
}
