# Example: Vue.js Frontend Audit

## Scenario

Auditing a Vue 3 frontend application with Pinia state management, Vue Router, and a REST API backend.

## Invocation

```text
Use the fuck-my-shit-mountain skill in maintainability mode.

Audit the Vue frontend at src/.
Target release: v1.0.0.
```

## Sample Finding (abbreviated from a real audit)

### Finding: API response type inconsistency forces scattered defensive casting

- Severity: Medium
- Confidence: High
- Category: Maintainability
- Status: Confirmed
- Affected area: API client layer and all consumers
- Evidence:
  - File: `src/api/users.ts:22`
  - Function / Module: `getUsers`
  - Relevant behavior:
    ```typescript
    async function getUsers(): Promise<User[]> {
      const response = await axios.get("/api/users");
      return response.data;
    }
    ```
    The function returns `response.data` typed as `User[]`, but `response.data` is `any` at runtime. There is no runtime validation of the API response shape.
  - Evidence from consumers:
    - File: `src/components/UserList.vue:35` — `const users = await getUsers(); users.map(u => u.name.toUpperCase())` — assumes `name` is string.
    - File: `src/composables/useUserStats.ts:12` — `users.filter(u => u.role === 'admin').length` — assumes `role` exists.
- Why this affects maintainability: Every consumer implicitly trusts the API response shape. If the API changes a field name, there is no compile-time or runtime error — the UI silently breaks. Fixing this requires finding all consumers.
- Risk of changing it: Adding runtime validation (e.g., Zod) would catch shape mismatches early but requires auditing all API functions and their consumers.
- Local fix: Add runtime validation at the API boundary:
  ```typescript
  import { z } from "zod";

  const UserSchema = z.object({
    id: z.number(),
    name: z.string(),
    email: z.string().email(),
    role: z.enum(["admin", "user", "viewer"]),
  });

  async function getUsers(): Promise<User[]> {
    const response = await axios.get("/api/users");
    return z.array(UserSchema).parse(response.data);
  }
  ```
- Better long-term fix: Adopt a patterns where all API responses are validated through schemas automatically (e.g., a validated axios wrapper).
- Test needed before refactor: Add integration tests that mock the API and verify UI behavior with known response shapes.
- Estimated effort: 2-3 days for full schema coverage across all endpoints

## Sample Finding 2

### Finding: Reactive state mutation outside of store actions

- Severity: Medium
- Confidence: High
- Category: Maintainability
- Status: Confirmed
- Affected area: State management
- Evidence:
  - File: `src/components/UserProfile.vue:28`
  - Relevant behavior:
    ```typescript
    const userStore = useUserStore();
    userStore.currentUser.name = newName; // Direct mutation outside store action
    ```
    The component directly mutates store state instead of calling a store action.
- Why this affects maintainability: Direct mutations bypass store actions, making it impossible to add side effects (validation, API calls, audit logging) in the future. It also makes debugging harder because state changes are not centralized.
- Risk of changing it: Low — the refactor is mechanical: move the mutation into a store action.
- Local fix:
  ```typescript
  // In store:
  updateUserName(name: string) {
    this.currentUser.name = name;
  }

  // In component:
  const userStore = useUserStore();
  userStore.updateUserName(newName);
  ```
- Better long-term fix: Enforce store action usage with a lint rule (e.g., `no-direct-store-mutation`).
- Test needed before refactor: Write a test that verifies `updateUserName` action works correctly.
- Estimated effort: 1 hour per affected component

## Key Takeaways for Vue Frontend Projects

1. Check for direct store mutations outside Pinia/Vuex actions.
2. Verify API boundary has runtime type validation (Zod, io-ts).
3. Check for missing `key` attributes in `v-for` loops.
4. Search for memory leaks — event listeners, watchers, intervals not cleaned up in `onUnmounted`.
5. Check component size — are single-file components over 400 lines?
6. Verify that `props` have type definitions and validators.
7. Check for `any` types in TypeScript Vue components.
8. Verify that async operations in composables handle loading and error states.
9. Check for missing `Suspense` or loading state in async components.
10. Verify that environment variables are validated on app startup, not assumed.
