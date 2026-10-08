# Example: Node.js Service Audit

## Scenario

Auditing a Node.js Express service with TypeScript, Redis caching, PostgreSQL database, and JWT-based authentication.

## Invocation

```text
Use the fuck-my-shit-mountain skill in full mode.

Audit the Node.js service at src/.
Target release: v2.0.0.
Focus on security and stability.
```

## Sample Finding (abbreviated from a real audit)

### Finding: JWT secret falls back to a hardcoded development value in production

- Severity: Critical
- Confidence: High
- Category: Security
- Status: Confirmed
- Affected area: Authentication configuration
- Evidence:
  - File: `src/config/auth.ts:15`
  - Function / Module: `getJwtSecret`
  - Relevant behavior:
    ```typescript
    export function getJwtSecret(): string {
      return process.env.JWT_SECRET || "dev-secret-do-not-use-in-production";
    }
    ```
    If `JWT_SECRET` environment variable is not set, the service silently falls back to a known hardcoded value.
- Attack precondition: The attacker knows the application uses this library or repository (public or leaked). They can generate valid JWTs with any payload.
- Attack path: Attacker generates a JWT signed with 'dev-secret-do-not-use-in-production'. Sends it to the service. The service accepts it as valid. Attacker gains any role they claim.
- Impact: Complete authentication bypass. Attacker can impersonate any user, including admin.
- Mitigation: Make `JWT_SECRET` required. Crash on startup if not set:
  ```typescript
  export function getJwtSecret(): string {
    const secret = process.env.JWT_SECRET;
    if (!secret) {
      throw new Error("JWT_SECRET environment variable is required");
    }
    return secret;
  }
  ```
- Regression test suggestion:
  ```typescript
  describe("getJwtSecret", () => {
    it("should throw if JWT_SECRET is not set", () => {
      delete process.env.JWT_SECRET;
      expect(() => getJwtSecret()).toThrow("JWT_SECRET environment variable is required");
    });

    it("should return the secret if set", () => {
      process.env.JWT_SECRET = "my-secret";
      expect(getJwtSecret()).toBe("my-secret");
    });
  });
  ```
- Estimated effort: 10 minutes

## Key Takeaways for Node.js Projects

1. Check for hardcoded secrets and unsafe fallbacks in config.
2. Search for `any` types — they bypass TypeScript safety.
3. Check error handling in async Express handlers — are they wrapped?
4. Verify that `process.exit()` is not called in library code.
5. Check for unvalidated redirects in auth flows.
6. Verify SQL parameterization — no string interpolation in queries.
7. Check for missing rate limiting on auth endpoints.
8. Search for `eval()`, `new Function()`, `vm.runInContext()`.
9. Check dependency age and known vulnerabilities.
10. Verify that `package.json` has `"engines"` field set.
