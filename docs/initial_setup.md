# Initial Setup

This guide prepares a Windows instrument PC or a macOS development machine for KohdaLab TRKR.

## 1. Install Tools

### Windows PowerShell

Install Git, GitHub CLI, and uv:

```powershell
winget install --id Git.Git -e --source winget
winget install --id GitHub.cli -e --source winget
winget install --id astral-sh.uv -e --source winget
```

Close and reopen PowerShell, then check:

```powershell
git --version
gh --version
uv --version
```

### macOS

Install Homebrew if needed, then install the tools:

```bash
brew install git gh uv
```

Check:

```bash
git --version
gh --version
uv --version
```

## 2. Configure Git and GitHub

Set your Git identity once per machine:

```bash
git config --global user.name "Your Name"
git config --global user.email "your-email@example.com"
```

Log in to GitHub:

```bash
gh auth login
gh auth setup-git
```

Use the browser login flow when prompted. For private repositories, confirm that `gh auth status` shows an authenticated account with repository access.

## 3. Clone the Repository

For the KohdaLab organization repository:

```bash
gh repo clone Kohdalab/kohdalab-trkr
cd kohdalab-trkr
```

For the personal mirror:

```bash
gh repo clone woolen-yarn/kohdalab-trkr
cd kohdalab-trkr
```

## 4. Create the Python Environment

Install runtime, GUI, notebook, and development dependencies:

```bash
uv sync --all-extras --group dev --frozen
```

## 5. Verify the Environment

Run checks before connecting instruments:

```bash
uv run ruff check .
uv run pytest -q
uv run kohdalab-cli --help
```

## 6. Run the Tools

GUI:

```bash
uv run kohdalab-gui
```

CLI:

```bash
uv run kohdalab-cli --help
```

Notebook dependencies are installed through the `notebook` extra and development group. See [Usage guide](usage.md) and [API usage examples](api_usage.md) for practical examples.

## 7. Recommended Git Workflow

Work through pull requests rather than pushing directly to `main`:

```bash
git switch -c feature/my-change
git status
git add <files>
git commit -m "Describe the change"
git push -u origin feature/my-change
gh pr create
```

The `main` branch requires the Ubuntu and Windows checks to pass before merge.

## References

- uv installation: https://docs.astral.sh/uv/getting-started/installation/
- GitHub CLI: https://github.com/cli/cli
- Git downloads: https://git-scm.com/downloads
