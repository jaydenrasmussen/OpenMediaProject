<script lang="ts">
  import "../app.css";
  import { page } from "$app/stores";
  import { Button } from "$lib/components/ui/button";
</script>

<div class="min-h-screen bg-background">
  <header class="bg-primary text-primary-foreground border-b">
    <div class="container mx-auto px-4 py-4 flex justify-between items-center">
      <div class="text-xl font-bold">OpenMedia Project</div>
      <nav>
        <ul class="flex space-x-4">
          <li>
            <a
              href="/"
              class="hover:text-opacity-80 transition-colors"
              aria-current={$page.url.pathname === "/" ? "page" : undefined}
            >
              <Button
                variant={$page.url.pathname === "/" ? "secondary" : "ghost"}
              >
                Home
              </Button>
            </a>
          </li>
          <li>
            <a
              href="/discover"
              class="hover:text-opacity-80 transition-colors"
              aria-current={$page.url.pathname === "/discover"
                ? "page"
                : undefined}
            >
              <Button
                variant={$page.url.pathname === "/discover"
                  ? "secondary"
                  : "ghost"}
              >
                Discover
              </Button>
            </a>
          </li>
          <li>
            <a
              href="/movies"
              class="hover:text-opacity-80 transition-colors"
              aria-current={$page.url.pathname === "/movies" ? "page" : undefined}
            >
              <Button
                variant={$page.url.pathname === "/movies" ? "secondary" : "ghost"}
              >
                Movies
              </Button>
            </a>
          </li>
          <li>
            <a
              href="/tvshows"
              class="hover:text-opacity-80 transition-colors"
              aria-current={$page.url.pathname === "/tvshows" ? "page" : undefined}
            >
              <Button
                variant={$page.url.pathname === "/tvshows" ? "secondary" : "ghost"}
              >
                TV Shows
              </Button>
            </a>
          </li>
          <li>
            <a
              href="/search"
              class="hover:text-opacity-80 transition-colors"
              aria-current={$page.url.pathname === "/search"
                ? "page"
                : undefined}
            >
              <Button
                variant={$page.url.pathname === "/search"
                  ? "secondary"
                  : "ghost"}
              >
                Search
              </Button>
            </a>
          </li>
          <li>
            <a
              href="/library"
              class="hover:text-opacity-80 transition-colors"
              aria-current={$page.url.pathname === "/library"
                ? "page"
                : undefined}
            >
              <Button
                variant={$page.url.pathname === "/library"
                  ? "secondary"
                  : "ghost"}
              >
                Library
              </Button>
            </a>
          </li>
          <li>
            <a
              href="/wanted"
              class="hover:text-opacity-80 transition-colors"
              aria-current={$page.url.pathname === "/wanted"
                ? "page"
                : undefined}
            >
              <Button
                variant={$page.url.pathname === "/wanted"
                  ? "secondary"
                  : "ghost"}
              >
                Wanted
              </Button>
            </a>
          </li>
          <li>
            <a
              href="/indexers"
              class="hover:text-opacity-80 transition-colors"
              aria-current={$page.url.pathname === "/indexers"
                ? "page"
                : undefined}
            >
              <Button
                variant={$page.url.pathname === "/indexers"
                  ? "secondary"
                  : "ghost"}
              >
                Indexers
              </Button>
            </a>
          </li>
          <li>
            <a
              href="/history"
              class="hover:text-opacity-80 transition-colors"
              aria-current={$page.url.pathname === "/history"
                ? "page"
                : undefined}
            >
              <Button
                variant={$page.url.pathname === "/history"
                  ? "secondary"
                  : "ghost"}
              >
                History
              </Button>
            </a>
          </li>
          <li>
            <a
              href="/settings"
              class="hover:text-opacity-80 transition-colors"
              aria-current={$page.url.pathname === "/settings"
                ? "page"
                : undefined}
            >
              <Button
                variant={$page.url.pathname === "/settings"
                  ? "secondary"
                  : "ghost"}
              >
                Settings
              </Button>
            </a>
          </li>
          <li>
            <a
              href="/system"
              class="hover:text-opacity-80 transition-colors"
              aria-current={$page.url.pathname === "/system"
                ? "page"
                : undefined}
            >
              <Button
                variant={$page.url.pathname === "/system"
                  ? "secondary"
                  : "ghost"}
              >
                System
              </Button>
            </a>
          </li>
          <li>
            <a
              href="/users"
              class="hover:text-opacity-80 transition-colors"
              aria-current={$page.url.pathname === "/users"
                ? "page"
                : undefined}
            >
              <Button
                variant={$page.url.pathname === "/users"
                  ? "secondary"
                  : "ghost"}
              >
                Users
              </Button>
            </a>
          </li>
          <li>
            <a
              href="/about"
              class="hover:text-opacity-80 transition-colors"
              aria-current={$page.url.pathname === "/about"
                ? "page"
                : undefined}
            >
              <Button
                variant={$page.url.pathname === "/about"
                  ? "secondary"
                  : "ghost"}
              >
                About
              </Button>
            </a>
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
          <ul class="flex space-x-4">
            <li>
              <a href="/privacy" class="hover:underline">Privacy Policy</a>
            </li>
            <li>
              <a href="/terms" class="hover:underline">Terms of Service</a>
            </li>
          </ul>
        </div>
      </div>
    </div>
  </footer>
</div>
