"use client";

import { useState } from "react";
import { DownloadClients } from "./DownloadClients";
import { General } from "./General";
import { Indexers } from "./Indexers";
import { MediaManagement } from "./MediaManagement";
import { Profiles } from "./Profiles";

const TABS = [
  { id: "media", label: "Media management", view: MediaManagement },
  { id: "profiles", label: "Quality profiles", view: Profiles },
  { id: "indexers", label: "Indexers", view: Indexers },
  { id: "clients", label: "Download clients", view: DownloadClients },
  { id: "general", label: "General", view: General },
] as const;

export default function Settings() {
  const [tab, setTab] = useState<(typeof TABS)[number]["id"]>("media");
  const View = TABS.find((t) => t.id === tab)!.view;
  return (
    <>
      <h1>Settings</h1>
      <div className="tabs" role="tablist">
        {TABS.map((t) => (
          <button key={t.id} role="tab" className={tab === t.id ? "active" : ""} onClick={() => setTab(t.id)}>
            {t.label}
          </button>
        ))}
      </div>
      <View />
    </>
  );
}
