"use client";

import { useEffect, useState } from "react";

export default function Home() {
  const [status, setStatus] = useState("checking…");

  useEffect(() => {
    fetch("/api/v1/health")
      .then((r) => r.json())
      .then((j) => setStatus(`server ${j.status} (v${j.version})`))
      .catch(() => setStatus("server unreachable"));
  }, []);

  return (
    <main>
      <h1>OpenMediaProject</h1>
      <p>{status}</p>
    </main>
  );
}
