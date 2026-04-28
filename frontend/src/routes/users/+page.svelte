<script lang="ts">
  import { onMount } from "svelte";
  import axios from "axios";
  import { userApi } from "$lib/api";
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

  // Types
  interface User {
    id: string;
    username: string;
    email: string;
    first_name?: string;
    last_name?: string;
    is_active?: boolean;
  }

  // State
  let users: User[] = [];
  let loading = true;
  let error: string | null = null;

  // Form state for creating a user
  let newUser = {
    username: "",
    email: "",
    first_name: "",
    last_name: "",
  };
  let formVisible = false;
  let submitting = false;

  // Fetch users on component mount
  onMount(async () => {
    await fetchUsers();
  });

  // Function to fetch users
  async function fetchUsers() {
    loading = true;
    error = null;

    try {
      const result = await userApi.getUsers();
      if (result.error) {
        throw new Error(result.message || "Failed to fetch users");
      }
      users = result.data || [];
    } catch (e) {
      error =
        e instanceof Error ? e.message : "An error occurred fetching users";
      console.error("Error fetching users:", e);
    } finally {
      loading = false;
    }
  }

  // Function to create a new user
  async function createUser() {
    submitting = true;
    error = null;

    try {
      const result = await userApi.createUser(newUser);
      if (result.error) {
        throw new Error(result.message || "Failed to create user");
      }

      if (result.data) {
        users = [...users, result.data];
      }

      // Reset form
      newUser = { username: "", email: "", first_name: "", last_name: "" };
      formVisible = false;
    } catch (e) {
      error =
        e instanceof Error ? e.message : "An error occurred creating user";
      console.error("Error creating user:", e);
    } finally {
      submitting = false;
    }
  }

  // Function to delete a user
  async function deleteUser(id: string) {
    if (!confirm("Are you sure you want to delete this user?")) {
      return;
    }

    error = null;

    try {
      const result = await userApi.deleteUser(id);
      if (result.error) {
        throw new Error(result.message || "Failed to delete user");
      }

      // Remove user from the list
      users = users.filter((user) => user.id !== id);
    } catch (e) {
      error =
        e instanceof Error ? e.message : "An error occurred deleting user";
      console.error("Error deleting user:", e);
    }
  }
</script>

<svelte:head>
  <title>Users | OpenMedia Project</title>
</svelte:head>

<div class="space-y-8">
  <div class="flex justify-between items-center">
    <h1 class="text-3xl font-bold">Users</h1>
    <Button
      variant={formVisible ? "outline" : "default"}
      on:click={() => (formVisible = !formVisible)}
    >
      {formVisible ? "Cancel" : "Add User"}
    </Button>
  </div>

  {#if error}
    <div
      class="bg-destructive/10 border-l-4 border-destructive text-destructive p-4 rounded flex items-start"
      role="alert"
    >
      <p>{error}</p>
    </div>
  {/if}

  {#if formVisible}
    <Card>
      <CardHeader>
        <CardTitle>Add New User</CardTitle>
        <CardDescription>Create a new user account</CardDescription>
      </CardHeader>
      <form on:submit|preventDefault={createUser}>
        <CardContent class="space-y-4">
          <div class="grid w-full items-center gap-1.5">
            <Label for="username">Username*</Label>
            <Input
              type="text"
              id="username"
              bind:value={newUser.username}
              required
              placeholder="Enter username"
            />
          </div>

          <div class="grid w-full items-center gap-1.5">
            <Label for="email">Email*</Label>
            <Input
              type="email"
              id="email"
              bind:value={newUser.email}
              required
              placeholder="Enter email address"
            />
          </div>

          <div class="grid w-full items-center gap-1.5">
            <Label for="first_name">First Name</Label>
            <Input
              type="text"
              id="first_name"
              bind:value={newUser.first_name}
              placeholder="Enter first name"
            />
          </div>

          <div class="grid w-full items-center gap-1.5">
            <Label for="last_name">Last Name</Label>
            <Input
              type="text"
              id="last_name"
              bind:value={newUser.last_name}
              placeholder="Enter last name"
            />
          </div>
        </CardContent>
        <CardFooter class="flex justify-end space-x-2">
          <Button
            type="button"
            variant="outline"
            on:click={() => (formVisible = false)}
          >
            Cancel
          </Button>
          <Button type="submit" disabled={submitting}>
            {submitting ? "Creating..." : "Create User"}
          </Button>
        </CardFooter>
      </form>
    </Card>
  {/if}

  {#if loading}
    <div class="text-center py-8">
      <div
        class="inline-block animate-spin rounded-full h-8 w-8 border-b-2 border-primary"
      ></div>
      <p class="mt-2 text-muted-foreground">Loading users...</p>
    </div>
  {:else if users.length === 0}
    <Card>
      <CardContent class="text-center py-8">
        <p class="text-muted-foreground">No users found.</p>
      </CardContent>
    </Card>
  {:else}
    <Card>
      <div class="overflow-x-auto">
        <table class="min-w-full">
          <thead class="bg-muted/50">
            <tr>
              <th class="py-3 px-4 text-left">ID</th>
              <th class="py-3 px-4 text-left">Username</th>
              <th class="py-3 px-4 text-left">Email</th>
              <th class="py-3 px-4 text-left">Name</th>
              <th class="py-3 px-4 text-left">Actions</th>
            </tr>
          </thead>
          <tbody>
            {#each users as user (user.id)}
              <tr class="border-t hover:bg-muted/20">
                <td class="py-3 px-4 text-sm">{user.id}</td>
                <td class="py-3 px-4 font-medium">{user.username}</td>
                <td class="py-3 px-4">{user.email}</td>
                <td class="py-3 px-4">
                  {user.first_name || ""}
                  {user.last_name || ""}
                </td>
                <td class="py-3 px-4">
                  <div class="flex space-x-2">
                    <Button variant="ghost" size="sm" class="h-8 w-8 p-0">
                      <span class="sr-only">Edit</span>
                      <span>✎</span>
                    </Button>
                    <Button
                      variant="ghost"
                      size="sm"
                      class="h-8 w-8 p-0 hover:bg-destructive/10 hover:text-destructive"
                      on:click={() => deleteUser(user.id)}
                    >
                      <span class="sr-only">Delete</span>
                      <span>🗑️</span>
                    </Button>
                  </div>
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    </Card>
  {/if}
</div>
