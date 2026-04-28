<script lang="ts">
  import { onMount } from "svelte";
  import {
    Card,
    CardContent,
    CardHeader,
    CardTitle,
  } from "$lib/components/ui/card";
  import { Button } from "$lib/components/ui/button";
  import { Input } from "$lib/components/ui/input";
  import { Label } from "$lib/components/ui/label";
  import apiClient from "$lib/api/index";
  import axios from "axios";

  type Settings = Record<string, string>;

  let loading = true;
  let saving = false;
  let error = "";
  let successMsg = "";
  let settings: Settings = {};

  const TORRENT_CLIENT_TYPES = [
    "rutorrent",
    "transmission",
    "deluge",
    "qbittorrent",
    "sabnzbd",
  ];

  const QUALITY_OPTIONS = ["2160p", "1080p", "720p", "480p", "any"];

  async function load() {
    loading = true;
    error = "";
    try {
      const res = await apiClient.get("/v1/settings");
      settings = res.data ?? {};
    } catch (e) {
      error = axios.isAxiosError(e)
        ? e.response?.data?.message || e.message
        : "Failed to load settings";
    }
    loading = false;
  }

  async function save() {
    saving = true;
    error = "";
    successMsg = "";
    try {
      await apiClient.put("/v1/settings", settings);
      successMsg = "Settings saved successfully.";
    } catch (e) {
      error = axios.isAxiosError(e)
        ? e.response?.data?.message || e.message
        : "Failed to save settings";
    }
    saving = false;
  }

  function field(key: string, label: string, type = "text", placeholder = "") {
    return { key, label, type, placeholder };
  }

  const TORRENT_FIELDS = [
    field("torrent_client_url", "Base URL", "url", "http://localhost:8080"),
    field("torrent_client_username", "Username"),
    field("torrent_client_password", "Password", "password"),
  ];

  const LIBRARY_FIELDS = [
    field("movie_dir", "Movie Directory", "text", "/media/movies"),
    field("tvshow_dir", "TV Show Directory", "text", "/media/tvshows"),
  ];

  const MEDIA_SERVER_FIELDS = [
    field("plex_url", "Plex URL", "url", "http://localhost:32400"),
    field("plex_token", "Plex Token", "password"),
    field("jellyfin_url", "Jellyfin URL", "url", "http://localhost:8096"),
    field("jellyfin_api_key", "Jellyfin API Key", "password"),
    field("emby_url", "Emby URL", "url", "http://localhost:8096"),
    field("emby_api_key", "Emby API Key", "password"),
  ];

  const NOTIFICATION_FIELDS = [
    field("discord_webhook_url", "Discord Webhook URL", "url"),
    field("ntfy_topic_url", "ntfy Topic URL", "url"),
    field("gotify_url", "Gotify URL", "url"),
    field("gotify_token", "Gotify Token", "password"),
    field("apprise_url", "Apprise URL", "url"),
    field("email_smtp_host", "SMTP Host"),
    field("email_smtp_port", "SMTP Port", "number"),
    field("email_username", "SMTP Username"),
    field("email_password", "SMTP Password", "password"),
    field("email_from", "From Address", "email"),
    field("email_to", "To Addresses (comma-separated)"),
  ];

  const INDEXER_FIELDS = [
    field("indexer_url", "Indexer URL", "url"),
    field("indexer_api_key", "Indexer API Key", "password"),
    field("indexer_name", "Indexer Name", "text", "Prowlarr"),
  ];

  onMount(load);
</script>

<svelte:head>
  <title>Settings — OpenMedia</title>
</svelte:head>

<div class="space-y-8 max-w-3xl mx-auto">
  <div class="flex items-center justify-between">
    <div>
      <h1 class="text-3xl font-bold">Settings</h1>
      <p class="text-sm text-muted-foreground mt-1">
        Application configuration. Changes are persisted across restarts.
      </p>
    </div>
    <div class="flex gap-2">
      <Button on:click={load} disabled={loading} variant="outline">
        {loading ? "Loading…" : "Reload"}
      </Button>
      <Button on:click={save} disabled={saving || loading}>
        {saving ? "Saving…" : "Save"}
      </Button>
    </div>
  </div>

  {#if error}
    <div class="rounded bg-red-50 border border-red-200 text-red-700 px-4 py-3 text-sm">{error}</div>
  {/if}
  {#if successMsg}
    <div class="rounded bg-green-50 border border-green-200 text-green-700 px-4 py-3 text-sm">{successMsg}</div>
  {/if}

  {#if loading}
    <div class="text-center py-20 text-muted-foreground">Loading…</div>
  {:else}
    <!-- Torrent Client -->
    <Card>
      <CardHeader><CardTitle>Torrent Client</CardTitle></CardHeader>
      <CardContent class="space-y-4">
        <div class="space-y-1">
          <Label for="torrent_client_type">Client Type</Label>
          <select
            id="torrent_client_type"
            bind:value={settings["torrent_client_type"]}
            class="w-full border rounded px-3 py-2 text-sm bg-background"
          >
            {#each TORRENT_CLIENT_TYPES as t}
              <option value={t}>{t}</option>
            {/each}
          </select>
        </div>
        {#each TORRENT_FIELDS as f}
          <div class="space-y-1">
            <Label for={f.key}>{f.label}</Label>
            <Input
              id={f.key}
              type={f.type}
              placeholder={f.placeholder}
              bind:value={settings[f.key]}
            />
          </div>
        {/each}
      </CardContent>
    </Card>

    <!-- Library Paths -->
    <Card>
      <CardHeader><CardTitle>Library Paths</CardTitle></CardHeader>
      <CardContent class="space-y-4">
        {#each LIBRARY_FIELDS as f}
          <div class="space-y-1">
            <Label for={f.key}>{f.label}</Label>
            <Input
              id={f.key}
              type={f.type}
              placeholder={f.placeholder}
              bind:value={settings[f.key]}
            />
          </div>
        {/each}
      </CardContent>
    </Card>

    <!-- Quality -->
    <Card>
      <CardHeader><CardTitle>Download Quality</CardTitle></CardHeader>
      <CardContent class="space-y-4">
        <div class="space-y-1">
          <Label for="preferred_quality">Preferred Quality</Label>
          <select
            id="preferred_quality"
            bind:value={settings["preferred_quality"]}
            class="w-full border rounded px-3 py-2 text-sm bg-background"
          >
            {#each QUALITY_OPTIONS as q}
              <option value={q}>{q}</option>
            {/each}
          </select>
        </div>
        <div class="space-y-1">
          <Label for="quality_fallback">Quality Fallback Order (comma-separated)</Label>
          <Input
            id="quality_fallback"
            type="text"
            placeholder="1080p,720p,any"
            bind:value={settings["quality_fallback"]}
          />
        </div>
      </CardContent>
    </Card>

    <!-- Media Servers -->
    <Card>
      <CardHeader><CardTitle>Media Servers</CardTitle></CardHeader>
      <CardContent class="space-y-4">
        {#each MEDIA_SERVER_FIELDS as f}
          <div class="space-y-1">
            <Label for={f.key}>{f.label}</Label>
            <Input
              id={f.key}
              type={f.type}
              placeholder={f.placeholder}
              bind:value={settings[f.key]}
            />
          </div>
        {/each}
      </CardContent>
    </Card>

    <!-- Indexers -->
    <Card>
      <CardHeader><CardTitle>Indexer</CardTitle></CardHeader>
      <CardContent class="space-y-4">
        {#each INDEXER_FIELDS as f}
          <div class="space-y-1">
            <Label for={f.key}>{f.label}</Label>
            <Input
              id={f.key}
              type={f.type}
              placeholder={f.placeholder}
              bind:value={settings[f.key]}
            />
          </div>
        {/each}
      </CardContent>
    </Card>

    <!-- Notifications -->
    <Card>
      <CardHeader><CardTitle>Notifications</CardTitle></CardHeader>
      <CardContent class="space-y-4">
        {#each NOTIFICATION_FIELDS as f}
          <div class="space-y-1">
            <Label for={f.key}>{f.label}</Label>
            <Input
              id={f.key}
              type={f.type}
              placeholder={f.placeholder}
              bind:value={settings[f.key]}
            />
          </div>
        {/each}
      </CardContent>
    </Card>

    <div class="flex justify-end">
      <Button on:click={save} disabled={saving}>
        {saving ? "Saving…" : "Save Settings"}
      </Button>
    </div>
  {/if}
</div>
