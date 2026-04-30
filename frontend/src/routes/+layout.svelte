<script lang="ts">
  import "../app.css";
  import { page } from "$app/stores";
  import { goto, afterNavigate } from "$app/navigation";
  import { Button } from "$lib/components/ui/button";
  import { onMount } from "svelte";

  let isLoggedIn = false;
  let username = "";

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

  onMount(readAuthState);
  afterNavigate(readAuthState);

  function isActive(path: string) {
    if (path === "/") return $page.url.pathname === "/";
    return $page.url.pathname.startsWith(path);
  }

  const navLinks = [
    { href: "/", label: "Home" },
    { href: "/discover", label: "Discover" },
    { href: "/movies", label: "Movies" },
    { href: "/tvshows", label: "TV Shows" },
    { href: "/search", label: "Search" },
    { href: "/library", label: "Library" },
    { href: "/downloads", label: "Downloads" },
    { href: "/wanted", label: "Wanted" },
    { href: "/indexers", label: "Indexers" },
    { href: "/history", label: "History" },
    { href: "/settings", label: "Settings" },
    { href: "/system", label: "System" },
    { href: "/users", label: "Users" },
  ];
</script>

<div class="min-h-screen bg-background">
  <header class="bg-primary text-primary-foreground border-b">
    <div class="container mx-auto px-4 py-4 flex justify-between items-center">
      <div class="text-xl font-bold flex-shrink-0">OpenMedia Project</div>
      <nav class="overflow-x-auto">
        <ul class="flex space-x-1 items-center">
          {#each navLinks as link}
            <li class="flex-none">
              <a
                href={link.href}
                class="hover:text-opacity-80 transition-colors"
                aria-current={isActive(link.href) ? "page" : undefined}
              >
                <Button
                  variant={isActive(link.href) ? "secondary" : "ghost"}
                  size="sm"
                >
                  {link.label}
                </Button>
              </a>
            </li>
          {/each}

          <!-- Login / Logout -->
          <li class="flex-none ml-2 border-l border-primary-foreground/20 pl-2">
            {#if isLoggedIn}
              <Button variant="ghost" size="sm" on:click={logout}>
                Logout{username ? ` (${username})` : ""}
              </Button>
            {:else}
              <a href="/login">
                <Button
                  variant={isActive("/login") ? "secondary" : "ghost"}
                  size="sm"
                >
                  Login
                </Button>
              </a>
            {/if}
          </li>
        </ul>
      </nav>
    </div>
  </header>

  <main class="container mx-auto px-4 py-8">
    <slot />
  </main>

  <footer class="bg-muted text-muted-foreground py-6">
    <div class="container mx-auto px-4">
      <div class="flex justify-between items-center">
        <div>
          <p>&copy; {new Date().getFullYear()} OpenMedia Project</p>
        </div>
        <div>
          <a href="/about" class="text-sm hover:underline">About</a>
        </div>
      </div>
    </div>
  </footer>
</div>
