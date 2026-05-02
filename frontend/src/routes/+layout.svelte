<script lang="ts">
  import "../app.css";
  import { page } from "$app/stores";
  import { goto, afterNavigate } from "$app/navigation";
  import { Button } from "$lib/components/ui/button";
  import { onMount } from "svelte";
  import { fly } from "svelte/transition";

  let isLoggedIn = false;
  let username = "";
  let mobileOpen = false;

  function readAuthState() {
    if (typeof localStorage !== "undefined") {
      isLoggedIn = !!localStorage.getItem("auth_token");
      username = localStorage.getItem("auth_username") ?? "";
    }
  }

  function logout() {
    localStorage.removeItem("auth_token");
    localStorage.removeItem("auth_username");
    localStorage.removeItem("auth_role");
    localStorage.removeItem("auth_user_id");
    isLoggedIn = false;
    username = "";
    goto("/login");
  }

  onMount(() => {
    readAuthState();
    if (!isLoggedIn && $page.url.pathname !== "/login") {
      goto("/login");
    }
  });
  afterNavigate(() => {
    readAuthState();
    mobileOpen = false;
    if (!isLoggedIn && $page.url.pathname !== "/login") {
      goto("/login");
    }
  });

  function isActive(path: string) {
    if (path === "/") return $page.url.pathname === "/";
    return $page.url.pathname.startsWith(path);
  }

  const navGroups = [
    {
      label: "Browse",
      links: [
        { href: "/", label: "Home" },
        { href: "/discover", label: "Discover" },
        { href: "/movies", label: "Movies" },
        { href: "/tvshows", label: "TV Shows" },
        { href: "/search", label: "Search" },
        { href: "/library", label: "Library" },
      ],
    },
    {
      label: "Activity",
      links: [
        { href: "/downloads", label: "Downloads" },
        { href: "/wanted", label: "Wanted" },
        { href: "/history", label: "History" },
      ],
    },
    {
      label: "Admin",
      links: [
        { href: "/indexers", label: "Indexers" },
        { href: "/settings", label: "Settings" },
        { href: "/system", label: "System" },
        { href: "/users", label: "Users" },
      ],
    },
  ];

  let collapsedGroups = new Set<string>(["Admin"]);

  function toggleGroup(label: string) {
    if (collapsedGroups.has(label)) {
      collapsedGroups.delete(label);
    } else {
      collapsedGroups.add(label);
    }
    collapsedGroups = collapsedGroups;
  }
</script>

<!-- Root shell: full viewport, no scroll on the shell itself -->
<div class="flex h-screen overflow-hidden bg-background">
  <!-- ── Desktop sidebar ─────────────────────────────────────────── -->
  <aside class="hidden md:flex md:flex-col w-64 shrink-0 border-r bg-card">
    <!-- Logo -->
    <div class="flex items-center h-16 px-6 border-b shrink-0">
      <span class="font-bold text-lg tracking-tight">OpenMediaProject</span>
    </div>

    <!-- Nav links -->
    <nav class="flex-1 overflow-y-auto py-2 px-3">
      {#each navGroups as group, i}
        <div class={i > 0 ? "mt-4" : ""}>
          <button
            class="flex w-full items-center justify-between px-2 py-1 text-xs font-semibold text-muted-foreground uppercase tracking-wider hover:text-foreground transition-colors rounded"
            on:click={() => toggleGroup(group.label)}
          >
            {group.label}
            <svg
              xmlns="http://www.w3.org/2000/svg"
              width="12"
              height="12"
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              stroke-width="2.5"
              stroke-linecap="round"
              stroke-linejoin="round"
              class="transition-transform duration-200 {collapsedGroups.has(
                group.label,
              )
                ? '-rotate-90'
                : ''}"><polyline points="6 9 12 15 18 9" /></svg
            >
          </button>
          {#if !collapsedGroups.has(group.label)}
            <div class="space-y-0.5 mt-0.5">
              {#each group.links as link}
                <a
                  href={link.href}
                  aria-current={isActive(link.href) ? "page" : undefined}
                >
                  <Button
                    variant={isActive(link.href) ? "secondary" : "ghost"}
                    class="w-full justify-start"
                  >
                    {link.label}
                  </Button>
                </a>
              {/each}
            </div>
          {/if}
        </div>
      {/each}
    </nav>

    <!-- Auth at bottom -->
    <div class="shrink-0 border-t p-3">
      {#if isLoggedIn}
        <Button variant="ghost" class="w-full justify-start" on:click={logout}>
          Logout{username ? ` (${username})` : ""}
        </Button>
      {:else}
        <a href="/login">
          <Button
            variant={isActive("/login") ? "secondary" : "ghost"}
            class="w-full justify-start"
          >
            Login
          </Button>
        </a>
      {/if}
    </div>
  </aside>

  <!-- ── Main area (header + scrollable content) ─────────────────── -->
  <div class="flex flex-col flex-1 min-w-0 overflow-hidden">
    <!-- Mobile top bar -->
    <header
      class="md:hidden flex items-center h-16 px-4 border-b bg-card shrink-0"
    >
      <button
        on:click={() => (mobileOpen = true)}
        aria-label="Open navigation menu"
        class="p-2 -ml-2 rounded-md hover:bg-accent transition-colors"
      >
        <!-- Hamburger icon -->
        <svg
          xmlns="http://www.w3.org/2000/svg"
          width="22"
          height="22"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          stroke-width="2"
          stroke-linecap="round"
          stroke-linejoin="round"
          aria-hidden="true"
        >
          <line x1="3" y1="6" x2="21" y2="6" />
          <line x1="3" y1="12" x2="21" y2="12" />
          <line x1="3" y1="18" x2="21" y2="18" />
        </svg>
      </button>
      <span class="ml-3 font-bold text-base">OpenMedia</span>
    </header>

    <!-- Page content -->
    <main class="flex-1 overflow-y-auto p-4 md:p-8">
      <slot />
    </main>
  </div>
</div>

<!-- ── Mobile drawer (rendered outside the flex shell to overlay everything) -->
{#if mobileOpen}
  <!-- Backdrop -->
  <div
    class="fixed inset-0 z-50 bg-black/60 md:hidden"
    role="presentation"
    on:click={() => (mobileOpen = false)}
  ></div>

  <!-- Slide-in drawer -->
  <aside
    class="fixed inset-y-0 left-0 z-50 w-64 flex flex-col bg-card border-r md:hidden"
    transition:fly={{ x: -256, duration: 220, opacity: 1 }}
  >
    <!-- Drawer header -->
    <div class="flex items-center justify-between h-16 px-6 border-b shrink-0">
      <span class="font-bold text-base tracking-tight">OpenMedia</span>
      <button
        on:click={() => (mobileOpen = false)}
        aria-label="Close navigation menu"
        class="p-1.5 rounded-md hover:bg-accent transition-colors"
      >
        <!-- X icon -->
        <svg
          xmlns="http://www.w3.org/2000/svg"
          width="18"
          height="18"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          stroke-width="2"
          stroke-linecap="round"
          stroke-linejoin="round"
          aria-hidden="true"
        >
          <line x1="18" y1="6" x2="6" y2="18" />
          <line x1="6" y1="6" x2="18" y2="18" />
        </svg>
      </button>
    </div>

    <!-- Nav links -->
    <nav class="flex-1 overflow-y-auto py-2 px-3">
      {#each navGroups as group, i}
        <div class={i > 0 ? "mt-4" : ""}>
          <button
            class="flex w-full items-center justify-between px-2 py-1 text-xs font-semibold text-muted-foreground uppercase tracking-wider hover:text-foreground transition-colors rounded"
            on:click={() => toggleGroup(group.label)}
          >
            {group.label}
            <svg
              xmlns="http://www.w3.org/2000/svg"
              width="12"
              height="12"
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              stroke-width="2.5"
              stroke-linecap="round"
              stroke-linejoin="round"
              class="transition-transform duration-200 {collapsedGroups.has(
                group.label,
              )
                ? '-rotate-90'
                : ''}"><polyline points="6 9 12 15 18 9" /></svg
            >
          </button>
          {#if !collapsedGroups.has(group.label)}
            <div class="space-y-0.5 mt-0.5">
              {#each group.links as link}
                <a
                  href={link.href}
                  on:click={() => (mobileOpen = false)}
                  aria-current={isActive(link.href) ? "page" : undefined}
                >
                  <Button
                    variant={isActive(link.href) ? "secondary" : "ghost"}
                    class="w-full justify-start"
                  >
                    {link.label}
                  </Button>
                </a>
              {/each}
            </div>
          {/if}
        </div>
      {/each}
    </nav>

    <!-- Auth at bottom -->
    <div class="shrink-0 border-t p-3">
      {#if isLoggedIn}
        <Button
          variant="ghost"
          class="w-full justify-start"
          on:click={() => {
            mobileOpen = false;
            logout();
          }}
        >
          Logout{username ? ` (${username})` : ""}
        </Button>
      {:else}
        <a href="/login" on:click={() => (mobileOpen = false)}>
          <Button
            variant={isActive("/login") ? "secondary" : "ghost"}
            class="w-full justify-start"
          >
            Login
          </Button>
        </a>
      {/if}
    </div>
  </aside>
{/if}
