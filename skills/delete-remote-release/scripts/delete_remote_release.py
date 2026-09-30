#!/usr/bin/env python3
"""Delete a remote git tag and/or its GitHub release draft.

Usage:
  python delete_remote_release.py --tag v1.4.0 [--remote github] [--repo owner/repo]
                                  [--tag-only] [--draft-only] [--dry-run] [--yes]

Safety:
  - Default mode is DRY-RUN: it lists exactly what *would* be deleted and makes no changes.
  - Pass --yes to actually delete. Deletion of a remote tag and a draft release is IRREVERSIBLE.
  - --tag is always required; the script never guesses a target.

Token resolution (only needed for the GitHub release-draft delete):
  1. $GITHUB_TOKEN environment variable
  2. `gh auth token` (if the gh CLI is installed and authenticated)
  3. `git credential fill` for github.com
  4. a token stored in $HOME/.workbuddy/secrets/github_token
  If none is found, release-draft deletion is skipped with instructions; remote tag
  deletion (via `git push`) still runs if git has its own push credentials.

Note: deleting a git tag does NOT delete the GitHub Release object. A botched
release often leaves a draft Release behind, so the script deletes both when asked.
"""
import argparse
import json
import os
import subprocess
import sys
import urllib.request
import urllib.error

API_BASE = "https://api.github.com"


def run(cmd, check=True):
    res = subprocess.run(cmd, shell=False, capture_output=True, text=True)
    if check and res.returncode != 0:
        raise RuntimeError(
            "command failed (%d): %s\n%s"
            % (res.returncode, " ".join(cmd), res.stderr.strip())
        )
    return res


def resolve_remote(args):
    if args.remote:
        return args.remote
    remotes = run(["git", "remote"]).stdout.split()
    for preferred in ("github", "origin"):
        if preferred in remotes:
            return preferred
    if remotes:
        return remotes[0]
    return None


def resolve_repo(remote):
    out = run(["git", "remote", "get-url", remote]).stdout.strip()
    if out.startswith("https://"):
        path = out[len("https://"):]
        path = path.split("@", 1)[-1]
        path = path.split("/", 1)[1] if "/" in path else path
    elif "github.com" in out and out.startswith("git@"):
        path = out.split(":", 1)[1]
    else:
        path = out.rsplit("/", 1)[-1]
    return path.removesuffix(".git")


def resolve_token():
    if os.environ.get("GITHUB_TOKEN"):
        return os.environ["GITHUB_TOKEN"].strip()
    # gh CLI
    try:
        r = subprocess.run(["gh", "auth", "token"], capture_output=True, text=True)
        if r.returncode == 0 and r.stdout.strip():
            return r.stdout.strip()
    except FileNotFoundError:
        pass
    # git credential helper
    try:
        inp = "protocol=https\nhost=github.com\n"
        r = subprocess.run(
            ["git", "credential", "fill"], input=inp, capture_output=True, text=True
        )
        if r.returncode == 0:
            for line in r.stdout.splitlines():
                if line.startswith("password="):
                    return line[len("password=") :].strip()
    except Exception:
        pass
    # stored secrets file
    p = os.path.join(os.path.expanduser("~"), ".workbuddy", "secrets", "github_token")
    if os.path.isfile(p):
        with open(p) as f:
            t = f.read().strip()
            if t:
                return t
    return None


def api(method, url, token, data=None, accept_404=False):
    req = urllib.request.Request(url)
    req.method = method
    req.add_header("Authorization", "Bearer %s" % token)
    req.add_header("Accept", "application/vnd.github+json")
    req.add_header("X-GitHub-Api-Version", "2022-11-28")
    if data is not None:
        req.data = json.dumps(data).encode()
        req.add_header("Content-Type", "application/json")
    try:
        with urllib.request.urlopen(req, timeout=30) as resp:
            body = resp.read().decode()
            return json.loads(body) if body else {}
    except urllib.error.HTTPError as e:
        if accept_404 and e.code == 404:
            return None
        detail = e.read().decode() if e.fp else ""
        raise RuntimeError("GitHub API %s %s -> %d: %s" % (method, url, e.code, detail))


def list_releases(repo, token):
    releases = []
    for page in range(1, 6):
        url = "%s/repos/%s/releases?per_page=100&page=%d" % (API_BASE, repo, page)
        data = api("GET", url, token)
        if not data:
            break
        releases.extend(data)
        if len(data) < 100:
            break
    return releases


def delete_tag(remote, tag, dry_run):
    if dry_run:
        print("[dry-run] would run: git push %s --delete %s" % (remote, tag))
        return
    run(["git", "push", remote, "--delete", tag])
    print("[done] deleted remote tag %s on %s" % (tag, remote))


def delete_draft(repo, token, tag, dry_run):
    releases = list_releases(repo, token)
    matches = [r for r in releases if r.get("tag_name") == tag]
    drafts = [r for r in matches if r.get("draft")]
    published = [r for r in matches if not r.get("draft")]

    if not matches:
        print("[info] no GitHub release found with tag_name=%s" % tag)
        return
    if published:
        print(
            "[warn] %d PUBLISHED (non-draft) release(s) with tag %s exist: %s"
            % (len(published), tag, ", ".join(r.get("name", "?") for r in published))
        )
        print(
            "        Deleting the tag will leave them orphaned. Published releases are NOT auto-deleted."
        )
    if not drafts:
        print("[info] no DRAFT release with tag_name=%s" % tag)
        return
    for r in drafts:
        rid = r["id"]
        name = r.get("name", "?")
        if dry_run:
            print(
                "[dry-run] would DELETE draft release id=%s name='%s' tag=%s"
                % (rid, name, tag)
            )
            continue
        api("DELETE", "%s/repos/%s/releases/%s" % (API_BASE, repo, rid), token)
        print("[done] deleted draft release id=%s name='%s'" % (rid, name))


def main():
    ap = argparse.ArgumentParser(
        description="Delete a remote git tag and/or its GitHub release draft."
    )
    ap.add_argument("--tag", required=True, help="tag name, e.g. v1.4.0 (required)")
    ap.add_argument("--remote", default=None, help="git remote (default: github, else origin)")
    ap.add_argument("--repo", default=None, help="owner/repo (default: derived from remote url)")
    ap.add_argument("--tag-only", action="store_true", help="only delete the remote tag")
    ap.add_argument("--draft-only", action="store_true", help="only delete the GitHub draft release")
    ap.add_argument("--dry-run", action="store_true", help="force dry-run (no changes)")
    ap.add_argument("--yes", action="store_true", help="actually perform deletions (default is dry-run)")
    args = ap.parse_args()

    dry_run = (not args.yes) or args.dry_run

    remote = resolve_remote(args)
    if remote is None:
        print("[error] no git remote found; pass --remote", file=sys.stderr)
        sys.exit(1)
    repo = args.repo or resolve_repo(remote)

    print("Target -> remote=%s tag=%s repo=%s" % (remote, args.tag, repo))

    if not args.draft_only:
        delete_tag(remote, args.tag, dry_run)

    if not args.tag_only:
        token = resolve_token()
        if not token:
            print(
                "[error] no GitHub token available for release-draft deletion.",
                file=sys.stderr,
            )
            print(
                "        Set $GITHUB_TOKEN, run `gh auth login`, configure `git credential fill`,",
                file=sys.stderr,
            )
            print(
                "        or put a token in ~/.workbuddy/secrets/github_token",
                file=sys.stderr,
            )
            print(
                "        (Remote tag deletion above, if any, still ran independently.)",
                file=sys.stderr,
            )
            sys.exit(2)
        delete_draft(repo, token, args.tag, dry_run)

    if dry_run:
        print("\n[dry-run] No changes were made. Re-run with --yes to actually delete.")


if __name__ == "__main__":
    main()
