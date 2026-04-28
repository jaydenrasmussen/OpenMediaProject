<script lang="ts">
  import {
    Card,
    CardContent,
    CardDescription,
    CardFooter,
    CardHeader,
    CardTitle,
  } from "$lib/components/ui/card";
  import { Button } from "$lib/components/ui/button";
</script>

<svelte:head>
  <title>About | OpenMedia Project</title>
  <meta name="description" content="About the OpenMedia Project" />
</svelte:head>

<div class="max-w-4xl mx-auto">
  <h1 class="text-3xl font-bold mb-6">About OpenMedia Project</h1>

  <Card class="mb-8">
    <CardHeader>
      <CardTitle>Project Overview</CardTitle>
      <CardDescription
        >A modern full-stack web application architecture</CardDescription
      >
    </CardHeader>
    <CardContent>
      <p class="mb-4">
        OpenMedia Project is a modern web application that demonstrates a
        full-stack architecture using:
      </p>
      <ul class="list-disc pl-5 mb-4 space-y-2">
        <li>
          <strong>Frontend:</strong> Static SvelteKit application with shadcn-svelte
          UI components
        </li>
        <li>
          <strong>Backend:</strong> Golang API built with the Fiber framework
        </li>
        <li>
          <strong>Database:</strong> Embedded NutsDB (pure Go key/value store)
        </li>
        <li><strong>Deployment:</strong> Single Docker container deployment</li>
      </ul>
      <p>
        The project is designed to be a starting point for building media
        management applications with a focus on simplicity, performance, and
        ease of deployment.
      </p>
    </CardContent>
  </Card>

  <Card class="mb-8">
    <CardHeader>
      <CardTitle>Tech Stack Details</CardTitle>
    </CardHeader>
    <CardContent class="space-y-6">
      <div>
        <h3 class="text-lg font-medium mb-2">Frontend</h3>
        <p class="mb-2">The frontend is built as a static site with:</p>
        <ul class="list-disc pl-5 space-y-1">
          <li>
            SvelteKit - A framework for building web applications of all sizes
          </li>
          <li>
            shadcn-svelte - A collection of accessible and customizable UI
            components
          </li>
          <li>TailwindCSS - A utility-first CSS framework</li>
          <li>Static site generation - For improved performance and SEO</li>
          <li>Client-side routing - For a smooth user experience</li>
        </ul>
      </div>

      <div>
        <h3 class="text-lg font-medium mb-2">Backend</h3>
        <p class="mb-2">The backend API is powered by:</p>
        <ul class="list-disc pl-5 space-y-1">
          <li>Go (Golang) - A statically typed, compiled language</li>
          <li>Fiber - A fast, Express-inspired web framework</li>
          <li>NutsDB - An embedded key/value database written in pure Go</li>
          <li>Repository pattern - For clean separation of concerns</li>
        </ul>
      </div>

      <div>
        <h3 class="text-lg font-medium mb-2">Deployment</h3>
        <p class="mb-2">The application is packaged for easy deployment:</p>
        <ul class="list-disc pl-5 space-y-1">
          <li>
            Single Docker container - Backend serves both API and static
            frontend
          </li>
          <li>Environment variables - For easy configuration</li>
          <li>Volume mounting - For persistent data storage</li>
        </ul>
      </div>
    </CardContent>
  </Card>

  <Card>
    <CardHeader>
      <CardTitle>Getting Started</CardTitle>
      <CardDescription>Explore the application features</CardDescription>
    </CardHeader>
    <CardContent>
      <p class="mb-4">You can explore this project by:</p>
      <ul class="list-disc pl-5 space-y-2">
        <li>
          Viewing the <a href="/" class="text-primary hover:underline"
            >home page</a
          >
        </li>
        <li>
          Checking out the <a href="/users" class="text-primary hover:underline"
            >users management</a
          >
        </li>
        <li>
          Exploring the API at <code class="bg-muted px-2 py-1 rounded"
            >/api/health</code
          >
          and <code class="bg-muted px-2 py-1 rounded">/api/v1/users</code>
        </li>
      </ul>

      <div class="mt-6">
        <p class="mb-2">
          For more information, please see the project repository documentation.
        </p>
      </div>
    </CardContent>
    <CardFooter>
      <Button as="a" href="/">Return to Home</Button>
    </CardFooter>
  </Card>
</div>
