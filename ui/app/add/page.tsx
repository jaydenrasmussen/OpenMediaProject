"use client";

import { AddSearch } from "../components/AddSearch";

export default function AddMovie() {
  return (
    <>
      <h1>Add movie</h1>
      <AddSearch kind="movie" placeholder="Title, “title year”, tmdb:603 or imdb:tt0133093" />
    </>
  );
}
