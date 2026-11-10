/**
 * Branch-ref resolution for the release scripts.
 *
 * `git rev-parse --abbrev-ref HEAD` returns the disambiguated `heads/<name>`
 * whenever a tag shares the branch name. The release branch and the release
 * tag are both the version string, so that is the normal case here, not an
 * edge case. Concatenating `refs/heads/${branch}` with that disambiguated
 * value yields `refs/heads/heads/<name>`, which matches no ref: the printed
 * push command fails, and the rebase-failure recovery path would create a
 * garbage `refs/heads/heads/*` ref while reporting the branch as restored.
 *
 * The short name is only safe for `git checkout`, where a full ref path would
 * detach HEAD instead of attaching the branch.
 */
export function resolveBranchRefs(abbreviatedRef) {
  const branch = String(abbreviatedRef)
    .trim()
    .replace(/^heads\//, "");
  return { branch, ref: `refs/heads/${branch}` };
}
