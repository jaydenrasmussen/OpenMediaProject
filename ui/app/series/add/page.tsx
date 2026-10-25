"use client";

import { AddSearch } from "../../components/AddSearch";

export default function AddSeries() {
  return (
    <>
      <h1>Add series</h1>
      <AddSearch kind="tv" placeholder="Title, tmdb:1396, tvdb:81189 or imdb:tt0903747" />
    </>
  );
}
