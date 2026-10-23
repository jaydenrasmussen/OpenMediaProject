"use client";

import { useState } from "react";
import { CustomFormats } from "./CustomFormats";
import { DownloadClients } from "./DownloadClients";
import { General } from "./General";
import { Indexers } from "./Indexers";
import { Lists } from "./Lists";
import { MediaManagement } from "./MediaManagement";
import { Notifications } from "./Notifications";
import { Plugins } from "./Plugins";
import { Profiles } from "./Profiles";
import { QualitySizes } from "./QualitySizes";
import { Rules } from "./Rules";

const TABS = [
  { id: "media", label: "Media management", view: MediaManagement },
  { id: "profiles", label: "Quality profiles", view: Profiles },
  { id: "formats", label: "Custom formats", view: CustomFormats },
  { id: "sizes", label: "Quality sizes", view: QualitySizes },
  { id: "rules", label: "Delays & tags", view: Rules },
  { id: "indexers", label: "Indexers", view: Indexers },
  { id: "clients", label: "Download clients", view: DownloadClients },
  { id: "lists", label: "Lists", view: Lists },
  { id: "notifications", label: "Notifications", view: Notifications },
  { id: "plugins", label: "Plugins", view: Plugins },
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
