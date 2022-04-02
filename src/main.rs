use chrono::{prelude::*, Duration};
use git2::{BranchType, Repository};
use std::io::{self, Bytes, Read, Stdin, Write};

fn main() {
    let result = (|| -> Result<_> {
        let repo = Repository::open_from_env()?;

        crossterm::terminal::enable_raw_mode()?;

        let mut stdout = io::stdout();
        let mut stdin = io::stdin().bytes();

        let branches = get_branches(&repo)?;

        if branches.is_empty() {
            write!(stdout, "No branches found (master ignored).\r\n")?;
        } else {
            let mut deleted_branch: Option<Branch> = None;

            for branch in branches {
                act_on_branch(branch, &mut stdout, &mut stdin, &mut deleted_branch, &repo)?;
            }
        }

        Ok(())
    })();

    crossterm::terminal::disable_raw_mode().ok();

    match result {
        Ok(_) => {}
        Err(e) => {
            eprintln!("{}", e);
            std::process::exit(1);
        }
    }
}

fn act_on_branch<'a>(
    mut branch: Branch<'a>,
    stdout: &mut std::io::Stdout,
    stdin: &mut Bytes<Stdin>,
    deleted_branch: &mut Option<Branch<'a>>,
    repo: &Repository,
) -> Result<()> {
    if branch.is_head {
        write!(
            stdout,
            "Ignoring '{}' because it is the current branch\r\n",
            branch.name
        )?;
    } else {
        match get_branch_action_from_user(stdout, stdin, &branch)? {
            BranchAction::Quit => {
                write!(stdout, "Quitting...\r\n")?;
                return Ok(());
            }
            BranchAction::Keep => {
                write!(stdout, "")?;
            }
            BranchAction::Delete => {
                branch.delete()?;

                write!(
                    stdout,
                    "Deleted branch '{}', to undo select 'u'\r\n",
                    branch.name
                )?;
                *deleted_branch = Some(branch);
            }
            BranchAction::Undo => {
                if let Some(branch) = &deleted_branch {
                    write!(stdout, "Undoing deletion of branch '{}'\r\n", branch.name)?;

                    let commit = repo.find_commit(branch.id)?;

                    repo.branch(&branch.name, &commit, false)?;
                } else {
                    write!(stdout, "No branch to undo deletion of\r\n")?;
                }
                *deleted_branch = None;

                act_on_branch(branch, stdout, stdin, deleted_branch, repo)?;
            }
        }
    }

    Ok(())
}

fn get_branch_action_from_user(
    stdout: &mut std::io::Stdout,
    stdin: &mut Bytes<Stdin>,
    branch: &Branch,
) -> Result<BranchAction> {
    write!(
        stdout,
        "'{}' ({}) last commit at {} (k/d/q/u/?) > ",
        branch.name,
        &branch.id.to_string()[..7],
        branch.time
    )?;
    stdout.flush()?;

    let byte = match stdin.next() {
        Some(byte) => byte?,
        None => return get_branch_action_from_user(stdout, stdin, branch),
    };

    let c = char::from(byte);
    write!(stdout, "{}\r\n", c)?;

    if c == '?' {
        write!(stdout, "Here are what the commands mean\r\n")?;
        write!(stdout, "k - Keep the branch\r\n")?;
        write!(stdout, "d - Delete the branch\r\n")?;
        write!(stdout, "u - Undo last deleted branch\r\n")?;
        write!(stdout, "q - Quit\r\n")?;
        write!(stdout, "? - Show this help text\r\n")?;
        stdout.flush()?;
        get_branch_action_from_user(stdout, stdin, branch)
    } else {
        BranchAction::try_from(c)
    }
}

fn get_branches(repo: &Repository) -> Result<Vec<Branch>> {
    let mut branches = repo
        .branches(Some(BranchType::Local))?
        .map(|branch| {
            let (branch, _) = branch?;
            let branch_name = branch.name_bytes()?;

            let commit = branch.get().peel_to_commit()?;

            let time = commit.time();
            let offset = Duration::minutes(i64::from(time.offset_minutes()));
            let time = NaiveDateTime::from_timestamp(time.seconds(), 0) + offset;

            Ok(Branch {
                time,
                id: commit.id(),
                name: String::from_utf8(branch_name.to_vec())?,
                is_head: branch.is_head(),
                branch,
            })
        })
        .filter(|branch| {
            let name = &branch.as_ref().unwrap().name;
            name != "master"
        })
        .collect::<Result<Vec<_>>>()?;

    branches.sort_unstable_by_key(|branch| branch.time);

    Ok(branches)
}

type Result<T, E = Error> = std::result::Result<T, E>;

struct Branch<'repo> {
    time: NaiveDateTime,
    id: git2::Oid,
    name: String,
    is_head: bool,
    branch: git2::Branch<'repo>,
}

impl<'repo> Branch<'repo> {
    fn delete(&mut self) -> Result<()> {
        self.branch.delete().map_err(From::from)
    }
}

enum BranchAction {
    Quit,
    Delete,
    Keep,
    Undo,
}

#[derive(Debug, thiserror::Error)]
enum Error {
    #[error(transparent)]
    CrosstermError(#[from] crossterm::ErrorKind),

    #[error(transparent)]
    GitError(#[from] git2::Error),

    #[error(transparent)]
    FromUtf8Error(#[from] std::string::FromUtf8Error),

    #[error("\n\rInvalid input, Dont know what to do with '{0}'")]
    InvalidInput(char),
}

impl TryFrom<char> for BranchAction {
    type Error = Error;

    fn try_from(c: char) -> Result<Self> {
        match c {
            'q' => Ok(BranchAction::Quit),
            'd' => Ok(BranchAction::Delete),
            'k' => Ok(BranchAction::Keep),
            'u' => Ok(BranchAction::Undo),
            _ => Err(Error::InvalidInput(c)),
        }
    }
}
