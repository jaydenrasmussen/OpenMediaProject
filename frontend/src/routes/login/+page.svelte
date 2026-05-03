<script lang="ts">
  import { goto } from "$app/navigation";
  import { authApi } from "$lib/api";
  import {
    Card,
    CardContent,
    CardDescription,
    CardFooter,
    CardHeader,
    CardTitle,
  } from "$lib/components/ui/card";
  import { InteractiveHoverButton } from "$lib/components/magic/interactive-hover-button";
  import { Label } from "$lib/components/ui/label";
  import { Input } from "$lib/components/ui/input";

  let username = $state("admin");
  let password = $state("admin");
  let submitting = $state(false);
  let error: string | null = $state(null);

  async function handleLogin() {
    submitting = true;
    error = null;

    try {
      const result = await authApi.login(username, password);
      if (result.error) {
        throw new Error(result.message || "Login failed");
      }
      if (!result.data) {
        throw new Error("Login failed: no data returned");
      }

      const data = result.data;
      localStorage.setItem("auth_token", data.token);
      localStorage.setItem("auth_user_id", data.user_id);
      localStorage.setItem("auth_username", data.username);
      localStorage.setItem("auth_role", data.role);

      goto("/");
    } catch (e) {
      error = e instanceof Error ? e.message : "An error occurred during login";
    } finally {
      submitting = false;
    }
  }
</script>

<svelte:head>
  <title>Login — OpenMediaProject</title>
</svelte:head>

<div class="flex justify-center items-center min-h-[60vh]">
  <Card class="w-full max-w-sm">
    <CardHeader>
      <CardTitle>Sign In</CardTitle>
      <CardDescription>Enter your credentials to access OpenMedia</CardDescription>
      <p class="text-xs text-muted-foreground pt-1">Default credentials on first run: <code>admin</code> / <code>admin</code></p>
    </CardHeader>

    <form onsubmit={(e) => { e.preventDefault(); handleLogin(); }}>
      <CardContent class="space-y-4">
        {#if error}
          <div
            class="bg-destructive/10 border-l-4 border-destructive text-destructive p-3 rounded text-sm"
            role="alert"
          >
            {error}
          </div>
        {/if}

        <div class="grid w-full items-center gap-1.5">
          <Label for="username">Username</Label>
          <Input
            type="text"
            id="username"
            bind:value={username}
            required
            placeholder="Enter username"
            autocomplete="username"
          />
        </div>

        <div class="grid w-full items-center gap-1.5">
          <Label for="password">Password</Label>
          <Input
            type="password"
            id="password"
            bind:value={password}
            required
            placeholder="Enter password"
            autocomplete="current-password"
          />
        </div>
      </CardContent>

      <CardFooter>
        <InteractiveHoverButton type="submit" class="w-full" disabled={submitting}>
          {submitting ? "Signing in..." : "Sign In"}
        </InteractiveHoverButton>
      </CardFooter>
    </form>
  </Card>
</div>
