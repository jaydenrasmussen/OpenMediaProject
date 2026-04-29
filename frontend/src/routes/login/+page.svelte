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
  import { Button } from "$lib/components/ui/button";
  import { Label } from "$lib/components/ui/label";
  import { Input } from "$lib/components/ui/input";

  let username = "";
  let password = "";
  let submitting = false;
  let error: string | null = null;

  async function handleLogin() {
    submitting = true;
    error = null;

    try {
      const result = await authApi.login(username, password);
      if (result.error) {
        throw new Error(result.message || "Login failed");
      }

      const data = result.data!;
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
  <title>Login | OpenMedia Project</title>
</svelte:head>

<div class="flex justify-center items-center min-h-[60vh]">
  <Card class="w-full max-w-sm">
    <CardHeader>
      <CardTitle>Sign In</CardTitle>
      <CardDescription>Enter your credentials to access OpenMedia</CardDescription>
    </CardHeader>

    <form on:submit|preventDefault={handleLogin}>
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
        <Button type="submit" class="w-full" disabled={submitting}>
          {submitting ? "Signing in..." : "Sign In"}
        </Button>
      </CardFooter>
    </form>
  </Card>
</div>
