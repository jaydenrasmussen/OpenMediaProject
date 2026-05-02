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
  import * as Accordion from "$lib/components/ui/accordion";
  import * as Select from "$lib/components/ui/select";
  import apiClient from "$lib/api/index";
  import axios from "axios";

  type Field = { key: string; label: string; type: string; placeholder: string };
  type Settings = Record<string, string>;

  let loading = true;
  let saving = false;
  let error = "";
  let successMsg = "";
  let settings: Settings = {};

  // Which media server / notification provider the user has selected
  let mediaServer = "";
  let notificationProvider = "";

  $: selectedMediaServerItem = mediaServer
    ? { value: mediaServer, label: MEDIA_SERVER_LABELS[mediaServer] }
    : undefined;

  $: selectedNotifItem = notificationProvider
    ? { value: notificationProvider, label: NOTIF_LABELS[notificationProvider] }
    : undefined;

  const MEDIA_SERVER_LABELS: Record<string, string> = {
    plex: "Plex",
    jellyfin: "Jellyfin",
    emby: "Emby",
  };

  const NOTIF_LABELS: Record<string, string> = {
    discord: "Discord",
    ntfy: "ntfy",
    gotify: "Gotify",
    apprise: "Apprise",
    telegram: "Telegram",
    email: "Email",
  };

  const TORRENT_CLIENT_TYPES = [
    "rutorrent",
    "transmission",
    "deluge",
    "qbittorrent",
    "sabnzbd",
  ];

  const QUALITY_OPTIONS = ["2160p", "1080p", "720p", "480p", "any"];

  function field(key: string, label: string, type = "text", placeholder = ""): Field {
    return { key, label, type, placeholder };
  }

  const TORRENT_FIELDS: Field[] = [
    field("torrent_client_url", "Base URL", "url", "http://localhost:8080"),
    field("torrent_client_username", "Username"),
    field("torrent_client_password", "Password", "password"),
  ];

  const LIBRARY_FIELDS: Field[] = [
    field("movie_dir", "Movie Directory", "text", "/media/movies"),
    field("tvshow_dir", "TV Show Directory", "text", "/media/tvshows"),
  ];

  const INDEXER_FIELDS: Field[] = [
    field("indexer_url", "Indexer URL", "url"),
    field("indexer_api_key", "Indexer API Key", "password"),
    field("indexer_name", "Indexer Name", "text", "Prowlarr"),
  ];

  const METADATA_FIELDS: Field[] = [
    field("tmdb_api_key", "TMDB API Key", "password"),
    field("tvdb_api_key", "TVDB API Key", "password"),
    field("omdb_api_key", "OMDB API Key", "password"),
  ];

  const MEDIA_SERVER_FIELDS: Record<string, Field[]> = {
    plex: [
      field("plex_url", "Plex URL", "url", "http://localhost:32400"),
      field("plex_token", "Plex Token", "password"),
    ],
    jellyfin: [
      field("jellyfin_url", "Jellyfin URL", "url", "http://localhost:8096"),
      field("jellyfin_api_key", "Jellyfin API Key", "password"),
    ],
    emby: [
      field("emby_url", "Emby URL", "url", "http://localhost:8096"),
      field("emby_api_key", "Emby API Key", "password"),
    ],
  };

  const NOTIFICATION_FIELDS: Record<string, Field[]> = {
    discord: [field("discord_webhook_url", "Webhook URL", "url")],
    ntfy: [field("ntfy_topic_url", "Topic URL", "url")],
    gotify: [
      field("gotify_url", "Gotify URL", "url"),
      field("gotify_token", "Token", "password"),
    ],
    apprise: [field("apprise_url", "Apprise URL", "url")],
    telegram: [
      field("telegram_bot_token", "Bot Token", "password"),
      field("telegram_chat_id", "Chat ID"),
    ],
    email: [
      field("email_smtp_host", "SMTP Host"),
      field("email_smtp_port", "SMTP Port", "number"),
      field("email_username", "SMTP Username"),
      field("email_password", "SMTP Password", "password"),
      field("email_from", "From Address", "email"),
      field("email_to", "To Addresses (comma-separated)"),
    ],
  };

  function inferSelections() {
    if (settings.plex_url || settings.plex_token) mediaServer = "plex";
    else if (settings.jellyfin_url || settings.jellyfin_api_key) mediaServer = "jellyfin";
    else if (settings.emby_url || settings.emby_api_key) mediaServer = "emby";
    else mediaServer = "";

    if (settings.discord_webhook_url) notificationProvider = "discord";
    else if (settings.ntfy_topic_url) notificationProvider = "ntfy";
    else if (settings.gotify_url || settings.gotify_token) notificationProvider = "gotify";
    else if (settings.apprise_url) notificationProvider = "apprise";
    else if (settings.telegram_bot_token || settings.telegram_chat_id) notificationProvider = "telegram";
    else if (settings.email_smtp_host) notificationProvider = "email";
    else notificationProvider = "";
  }

  async function load() {
    loading = true;
    error = "";
    try {
      const res = await apiClient.get("/v1/settings");
      settings = res.data ?? {};
      inferSelections();
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

  onMount(load);
</script>

<svelte:head>
  <title>Settings — OpenMedia</title>
</svelte:head>

<div class="space-y-6 max-w-3xl mx-auto">
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
            <Input id={f.key} type={f.type} placeholder={f.placeholder} bind:value={settings[f.key]} />
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
            <Input id={f.key} type={f.type} placeholder={f.placeholder} bind:value={settings[f.key]} />
          </div>
        {/each}
      </CardContent>
    </Card>

    <!-- Download Quality -->
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

    <!-- Indexer -->
    <Card>
      <CardHeader><CardTitle>Indexer</CardTitle></CardHeader>
      <CardContent class="space-y-4">
        {#each INDEXER_FIELDS as f}
          <div class="space-y-1">
            <Label for={f.key}>{f.label}</Label>
            <Input id={f.key} type={f.type} placeholder={f.placeholder} bind:value={settings[f.key]} />
          </div>
        {/each}
      </CardContent>
    </Card>

    <!-- Media Server -->
    <Card>
      <CardHeader>
        <CardTitle>Media Server</CardTitle>
      </CardHeader>
      <CardContent class="space-y-4">
        <div class="space-y-1">
          <Label>Server</Label>
          <Select.Root
            selected={selectedMediaServerItem}
            onSelectedChange={(v) => { mediaServer = v?.value ?? ""; }}
          >
            <Select.Trigger class="w-full">
              <Select.Value placeholder="Select a media server…" />
            </Select.Trigger>
            <Select.Content>
              <Select.Item value="plex" label="Plex">Plex</Select.Item>
              <Select.Item value="jellyfin" label="Jellyfin">Jellyfin</Select.Item>
              <Select.Item value="emby" label="Emby">Emby</Select.Item>
            </Select.Content>
          </Select.Root>
        </div>

        {#if mediaServer && MEDIA_SERVER_FIELDS[mediaServer]}
          <div class="space-y-4 pt-2">
            {#each MEDIA_SERVER_FIELDS[mediaServer] as f}
              <div class="space-y-1">
                <Label for={f.key}>{f.label}</Label>
                <Input id={f.key} type={f.type} placeholder={f.placeholder} bind:value={settings[f.key]} />
              </div>
            {/each}
          </div>
        {/if}
      </CardContent>
    </Card>

    <!-- Notifications -->
    <Card>
      <CardHeader>
        <CardTitle>Notifications</CardTitle>
      </CardHeader>
      <CardContent class="space-y-4">
        <div class="space-y-1">
          <Label>Provider</Label>
          <Select.Root
            selected={selectedNotifItem}
            onSelectedChange={(v) => { notificationProvider = v?.value ?? ""; }}
          >
            <Select.Trigger class="w-full">
              <Select.Value placeholder="Select a notification provider…" />
            </Select.Trigger>
            <Select.Content>
              <Select.Item value="discord" label="Discord">Discord</Select.Item>
              <Select.Item value="ntfy" label="ntfy">ntfy</Select.Item>
              <Select.Item value="gotify" label="Gotify">Gotify</Select.Item>
              <Select.Item value="apprise" label="Apprise">Apprise</Select.Item>
              <Select.Item value="telegram" label="Telegram">Telegram</Select.Item>
              <Select.Item value="email" label="Email">Email</Select.Item>
            </Select.Content>
          </Select.Root>
        </div>

        {#if notificationProvider && NOTIFICATION_FIELDS[notificationProvider]}
          <div class="space-y-4 pt-2">
            {#each NOTIFICATION_FIELDS[notificationProvider] as f}
              <div class="space-y-1">
                <Label for={f.key}>{f.label}</Label>
                <Input id={f.key} type={f.type} placeholder={f.placeholder} bind:value={settings[f.key]} />
              </div>
            {/each}
          </div>
        {/if}
      </CardContent>
    </Card>

    <!-- Metadata API Keys (accordion — most users won't need these) -->
    <Accordion.Root type="single">
      <Accordion.Item
        value="metadata"
        class="border rounded-lg bg-card text-card-foreground shadow-sm px-6"
      >
        <Accordion.Trigger class="text-base font-semibold w-full flex justify-between py-5 hover:no-underline">
          Metadata API Keys
          <span class="text-xs font-normal text-muted-foreground">TMDB · TVDB · OMDB</span>
        </Accordion.Trigger>
        <Accordion.Content>
          <p class="text-sm text-muted-foreground mb-4">
            TMDB and TVDB default keys are bundled. Add an OMDB key (free at omdbapi.com) to unlock IMDB, Rotten Tomatoes, and Metacritic scores on TV/movie detail pages.
          </p>
          <div class="space-y-4 pb-6">
            {#each METADATA_FIELDS as f}
              <div class="space-y-1">
                <Label for={f.key}>{f.label}</Label>
                <Input id={f.key} type={f.type} placeholder={f.placeholder} bind:value={settings[f.key]} />
              </div>
            {/each}
          </div>
        </Accordion.Content>
      </Accordion.Item>
    </Accordion.Root>

    <div class="flex justify-end">
      <Button on:click={save} disabled={saving}>
        {saving ? "Saving…" : "Save Settings"}
      </Button>
    </div>
  {/if}
</div>
