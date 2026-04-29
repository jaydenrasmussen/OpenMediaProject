<script lang="ts">
  import { onMount } from "svelte";
  import axios from "axios";
  import {
    Card,
    CardContent,
    CardDescription,
    CardFooter,
    CardHeader,
    CardTitle,
  } from "$lib/components/ui/card";
  import { Button } from "$lib/components/ui/button";

  // State for API status
  let apiStatus: "loading" | "online" | "offline" = "loading";
  let apiMessage: string = "";

  // Check the API status when the component mounts
  onMount(async () => {
    try {
      const baseUrl = import.meta.env.VITE_API_URL || "/api";
      const response = await axios.get(`${baseUrl}/health`);
      const data = response.data;

      if (data.status === "ok") {
        apiStatus = "online";
        apiMessage = data.message;
      } else {
        apiStatus = "offline";
      }
    } catch (error) {
      apiStatus = "offline";
      console.error("API connection error:", error);
    }
  });
</script>

<svelte:head>
  <title>OpenMedia Project</title>
  <meta name="description" content="A modern media management platform" />
</svelte:head>

<div class="space-y-10">
  <section class="text-center py-12">
    <h1 class="text-4xl md:text-6xl font-bold mb-4">
      Welcome to OpenMedia Project
    </h1>
    <p class="text-xl text-muted-foreground max-w-3xl mx-auto">
      A self-hosted media management platform. Browse, request, and automatically
      download movies and TV shows into your personal library.
    </p>
  </section>

  <Card class="max-w-4xl mx-auto">
    <CardHeader>
      <CardTitle>System Status</CardTitle>
      <CardDescription
        >Current status of the application services</CardDescription
      >
    </CardHeader>

    <CardContent>
      <div class="flex items-center space-x-4 mb-6">
        <div class="flex items-center">
          <span class="mr-2 font-medium">Backend API:</span>
          {#if apiStatus === "loading"}
            <div
              class="bg-yellow-100 text-yellow-800 px-3 py-1 rounded-full flex items-center"
            >
              <span class="w-4 h-4 mr-1.5 animate-pulse">⟳</span> Checking...
            </div>
          {:else if apiStatus === "online"}
            <div
              class="bg-green-100 text-green-800 px-3 py-1 rounded-full flex items-center"
            >
              <span class="w-4 h-4 mr-1.5">✓</span> Online
            </div>
          {:else}
            <div
              class="bg-red-100 text-red-800 px-3 py-1 rounded-full flex items-center"
            >
              <span class="w-4 h-4 mr-1.5">✗</span> Offline
            </div>
          {/if}
        </div>

        {#if apiStatus === "online" && apiMessage}
          <div class="text-sm text-muted-foreground">{apiMessage}</div>
        {/if}
      </div>
    </CardContent>

    <CardFooter>
      <div class="flex space-x-4">
        <!-- Button as="a" is invalid — the Button component always renders a
             <button> element; href on a <button> has no effect. Use an <a> wrapper. -->
        <a href="/discover"><Button>Browse Discover</Button></a>
        <a href="/movies"><Button variant="outline">Movie Library</Button></a>
      </div>
    </CardFooter>
  </Card>

  <section class="max-w-4xl mx-auto">
    <h2 class="text-2xl font-semibold mb-6">Features</h2>

    <div class="grid md:grid-cols-3 gap-6">
      <Card>
        <CardHeader>
          <CardTitle class="text-xl">Discover &amp; Request</CardTitle>
        </CardHeader>
        <CardContent>
          <p class="text-muted-foreground">
            Browse trending movies and popular TV shows powered by TMDB and
            TVDB. One click to request a download.
          </p>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle class="text-xl">Automatic Downloads</CardTitle>
        </CardHeader>
        <CardContent>
          <p class="text-muted-foreground">
            Searches all configured indexers concurrently, picks the best
            quality match, and sends it straight to your torrent client.
          </p>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle class="text-xl">Monitor TV Shows</CardTitle>
        </CardHeader>
        <CardContent>
          <p class="text-muted-foreground">
            Enable monitoring on any series to automatically queue new episodes
            as they air, with per-episode status tracking.
          </p>
        </CardContent>
      </Card>
    </div>
  </section>
</div>
