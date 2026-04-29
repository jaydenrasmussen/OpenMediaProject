import axios from "axios";

// Create an axios instance with common configuration
const apiClient = axios.create({
  baseURL: import.meta.env.VITE_API_URL || "/api", // Use environment variable or default to /api
  timeout: 10000,
  headers: {
    "Content-Type": "application/json",
  },
});

// Types
export interface User {
  id: string;
  username: string;
  email: string;
  first_name?: string;
  last_name?: string;
  is_active?: boolean;
  created_at?: string;
  updated_at?: string;
}

export interface ApiResponse<T> {
  data?: T;
  error?: boolean;
  message?: string;
}

// User API calls
export const userApi = {
  // Get all users
  getUsers: async (): Promise<ApiResponse<User[]>> => {
    try {
      const response = await apiClient.get("/v1/users");
      return { data: response.data.users };
    } catch (error) {
      if (axios.isAxiosError(error)) {
        return {
          error: true,
          message: error.response?.data?.message || error.message,
        };
      }
      return { error: true, message: "Unknown error occurred" };
    }
  },

  // Get a single user by ID
  getUser: async (id: string): Promise<ApiResponse<User>> => {
    try {
      const response = await apiClient.get(`/v1/users/${id}`);
      return { data: response.data };
    } catch (error) {
      if (axios.isAxiosError(error)) {
        return {
          error: true,
          message: error.response?.data?.message || error.message,
        };
      }
      return { error: true, message: "Unknown error occurred" };
    }
  },

  // Create a new user
  createUser: async (user: Omit<User, "id">): Promise<ApiResponse<User>> => {
    try {
      const response = await apiClient.post("/v1/users", user);
      return { data: response.data };
    } catch (error) {
      if (axios.isAxiosError(error)) {
        return {
          error: true,
          message: error.response?.data?.message || error.message,
        };
      }
      return { error: true, message: "Unknown error occurred" };
    }
  },

  // Update an existing user
  updateUser: async (
    id: string,
    user: Partial<User>,
  ): Promise<ApiResponse<User>> => {
    try {
      const response = await apiClient.put(`/v1/users/${id}`, user);
      return { data: response.data };
    } catch (error) {
      if (axios.isAxiosError(error)) {
        return {
          error: true,
          message: error.response?.data?.message || error.message,
        };
      }
      return { error: true, message: "Unknown error occurred" };
    }
  },

  // Delete a user
  deleteUser: async (id: string): Promise<ApiResponse<null>> => {
    try {
      await apiClient.delete(`/v1/users/${id}`);
      return { data: null };
    } catch (error) {
      if (axios.isAxiosError(error)) {
        return {
          error: true,
          message: error.response?.data?.message || error.message,
        };
      }
      return { error: true, message: "Unknown error occurred" };
    }
  },
};

// Interceptors
apiClient.interceptors.request.use(
  (config) => {
    // You can add auth token here if needed
    // const token = getToken();
    // if (token) {
    //   config.headers.Authorization = `Bearer ${token}`;
    // }
    return config;
  },
  (error) => {
    return Promise.reject(error);
  },
);

apiClient.interceptors.response.use(
  (response) => response,
  (error) => {
    // Handle global error responses here
    if (error.response?.status === 401) {
      // Handle unauthorized access
      // e.g., redirect to login or refresh token
    }

    // Special handling for static site in development - retry without baseURL if needed
    const originalRequest = error.config;
    if (
      error.message?.includes("Network Error") &&
      !originalRequest._retry &&
      originalRequest.url &&
      !originalRequest.url.startsWith("http")
    ) {
      originalRequest._retry = true;
      const fallbackUrl = `/api${originalRequest.url.startsWith("/") ? originalRequest.url : "/" + originalRequest.url}`;
      return axios(
        Object.assign({}, originalRequest, { baseURL: "", url: fallbackUrl }),
      );
    }

    return Promise.reject(error);
  },
);

export default apiClient;

// ─── Media Types ─────────────────────────────────────────────────────────────

export interface Movie {
  id: string;
  title: string;
  overview: string;
  release_date: string;
  runtime: number;
  poster_url: string;
  backdrop_url: string;
  rating: number;
  vote_count: number;
  genres: string[];
  director: string;
  popularity_index: number;
}

export interface TVShow {
  id: string;
  name: string;
  overview: string;
  first_aired: string;
  network: string;
  status: string;
  poster_url: string;
  backdrop_url: string;
  rating: number;
  genres: string[];
  monitored: boolean;
}

export interface Episode {
  id: string;
  show_id: string;
  season: number;
  number: number;
  title: string;
  air_date: string;
  overview: string;
  /** "wanted" | "downloading" | "downloaded" | "skipped" | "missing" */
  status: string;
  downloaded_at?: string;
  quality?: string;
}

export interface WantedItem {
  id: string;
  media_type: string;
  media_id: string;
  title: string;
  season?: number;
  episode?: number;
  min_quality: string;
  added_at: string;
  updated_at: string;
  status: string;
  added_by?: string;
}

export interface LibraryFile {
  path: string;
  title?: string;
  size: number;
  mod_time?: string;
}

// ─── Movie API ────────────────────────────────────────────────────────────────

export const movieApi = {
  getMovies: async (): Promise<ApiResponse<{ movies: Movie[]; count: number }>> => {
    try {
      const response = await apiClient.get("/v1/movies");
      return { data: response.data };
    } catch (error) {
      if (axios.isAxiosError(error))
        return { error: true, message: error.response?.data?.message || error.message };
      return { error: true, message: "Unknown error occurred" };
    }
  },

  searchMovies: async (q: string): Promise<ApiResponse<{ movies: Movie[]; count: number }>> => {
    try {
      const response = await apiClient.get("/v1/movies/search", { params: { q } });
      return { data: response.data };
    } catch (error) {
      if (axios.isAxiosError(error))
        return { error: true, message: error.response?.data?.message || error.message };
      return { error: true, message: "Unknown error occurred" };
    }
  },

  getMovie: async (id: string): Promise<ApiResponse<Movie>> => {
    try {
      const response = await apiClient.get(`/v1/movies/${id}`);
      return { data: response.data };
    } catch (error) {
      if (axios.isAxiosError(error))
        return { error: true, message: error.response?.data?.message || error.message };
      return { error: true, message: "Unknown error occurred" };
    }
  },

  requestMovie: async (
    id: string,
    opts?: { min_quality?: string; upgrade_until?: string },
  ): Promise<ApiResponse<{ success: boolean; message: string; id: string }>> => {
    try {
      const response = await apiClient.post(`/v1/movies/${id}/request`, opts ?? {});
      return { data: response.data };
    } catch (error) {
      if (axios.isAxiosError(error))
        return { error: true, message: error.response?.data?.message || error.message };
      return { error: true, message: "Unknown error occurred" };
    }
  },

  fetchPopular: async (): Promise<ApiResponse<null>> => {
    try {
      await apiClient.post("/v1/movies/fetch-popular");
      return { data: null };
    } catch (error) {
      if (axios.isAxiosError(error))
        return { error: true, message: error.response?.data?.message || error.message };
      return { error: true, message: "Unknown error occurred" };
    }
  },

  fetchNowPlaying: async (): Promise<ApiResponse<null>> => {
    try {
      await apiClient.post("/v1/movies/fetch-now-playing");
      return { data: null };
    } catch (error) {
      if (axios.isAxiosError(error))
        return { error: true, message: error.response?.data?.message || error.message };
      return { error: true, message: "Unknown error occurred" };
    }
  },
};

// ─── TV API ───────────────────────────────────────────────────────────────────

export const tvApi = {
  getTVShows: async (): Promise<ApiResponse<{ tvshows: TVShow[]; count: number }>> => {
    try {
      const response = await apiClient.get("/v1/tvshows");
      return { data: response.data };
    } catch (error) {
      if (axios.isAxiosError(error))
        return { error: true, message: error.response?.data?.message || error.message };
      return { error: true, message: "Unknown error occurred" };
    }
  },

  searchTVShows: async (q: string): Promise<ApiResponse<{ tvshows: TVShow[]; count: number }>> => {
    try {
      const response = await apiClient.get("/v1/tvshows/search", { params: { q } });
      return { data: response.data };
    } catch (error) {
      if (axios.isAxiosError(error))
        return { error: true, message: error.response?.data?.message || error.message };
      return { error: true, message: "Unknown error occurred" };
    }
  },

  getTVShow: async (id: string): Promise<ApiResponse<TVShow>> => {
    try {
      const response = await apiClient.get(`/v1/tvshows/${id}`);
      return { data: response.data };
    } catch (error) {
      if (axios.isAxiosError(error))
        return { error: true, message: error.response?.data?.message || error.message };
      return { error: true, message: "Unknown error occurred" };
    }
  },

  requestTVShow: async (
    id: string,
    opts?: { season?: number; episode?: number; min_quality?: string; upgrade_until?: string },
  ): Promise<ApiResponse<{ success: boolean; message: string; id: string }>> => {
    try {
      const response = await apiClient.post(`/v1/tvshows/${id}/request`, opts ?? {});
      return { data: response.data };
    } catch (error) {
      if (axios.isAxiosError(error))
        return { error: true, message: error.response?.data?.message || error.message };
      return { error: true, message: "Unknown error occurred" };
    }
  },

  fetchPopularTV: async (): Promise<ApiResponse<null>> => {
    try {
      await apiClient.post("/v1/tvshows/fetch-popular");
      return { data: null };
    } catch (error) {
      if (axios.isAxiosError(error))
        return { error: true, message: error.response?.data?.message || error.message };
      return { error: true, message: "Unknown error occurred" };
    }
  },

  getEpisodes: async (id: string): Promise<ApiResponse<{ show_id: string; episodes: Episode[]; count: number }>> => {
    try {
      const response = await apiClient.get(`/v1/tvshows/${id}/episodes`);
      return { data: response.data };
    } catch (error) {
      if (axios.isAxiosError(error))
        return { error: true, message: error.response?.data?.message || error.message };
      return { error: true, message: "Unknown error occurred" };
    }
  },

  monitorTVShow: async (
    id: string,
  ): Promise<ApiResponse<{ success: boolean; monitored: boolean; episodes: number; queued: number }>> => {
    try {
      const response = await apiClient.post(`/v1/tvshows/${id}/monitor`);
      return { data: response.data };
    } catch (error) {
      if (axios.isAxiosError(error))
        return { error: true, message: error.response?.data?.message || error.message };
      return { error: true, message: "Unknown error occurred" };
    }
  },
};

// ─── Wanted API ───────────────────────────────────────────────────────────────

export const wantedApi = {
  getWanted: async (): Promise<ApiResponse<{ items: WantedItem[]; count: number }>> => {
    try {
      const response = await apiClient.get("/v1/wanted");
      return { data: response.data };
    } catch (error) {
      if (axios.isAxiosError(error))
        return { error: true, message: error.response?.data?.message || error.message };
      return { error: true, message: "Unknown error occurred" };
    }
  },

  deleteWanted: async (id: string): Promise<ApiResponse<null>> => {
    try {
      await apiClient.delete(`/v1/wanted/${id}`);
      return { data: null };
    } catch (error) {
      if (axios.isAxiosError(error))
        return { error: true, message: error.response?.data?.message || error.message };
      return { error: true, message: "Unknown error occurred" };
    }
  },
};

// ─── Library API ──────────────────────────────────────────────────────────────

export const libraryApi = {
  scan: async (): Promise<ApiResponse<{
    movies: { matched: LibraryFile[]; unmatched: LibraryFile[] };
    tvshows: { matched: LibraryFile[]; unmatched: LibraryFile[] };
    scanned_at: string;
  }>> => {
    try {
      const response = await apiClient.get("/v1/library/scan");
      return { data: response.data };
    } catch (error) {
      if (axios.isAxiosError(error))
        return { error: true, message: error.response?.data?.message || error.message };
      return { error: true, message: "Unknown error occurred" };
    }
  },
};

// ─── Indexer Health API ───────────────────────────────────────────────────────

export interface IndexerHealth {
  name: string;
  enabled: boolean;
  last_success: string;
  last_result_count: number;
  total_searches: number;
  total_errors: number;
  error_rate: number;
  avg_response_ms: number;
}

export const indexerApi = {
  getHealth: async (): Promise<ApiResponse<{ indexers: IndexerHealth[] }>> => {
    try {
      const response = await apiClient.get("/v1/indexers/health");
      return { data: response.data };
    } catch (error) {
      if (axios.isAxiosError(error))
        return { error: true, message: error.response?.data?.message || error.message };
      return { error: true, message: "Unknown error occurred" };
    }
  },
};

// ─── History API ──────────────────────────────────────────────────────────────

export interface HistoryEntry {
  id: string;
  event_type: string;
  title: string;
  media_type?: string;
  media_id?: string;
  message?: string;
  user?: string;
  created_at: string;
}

export const historyApi = {
  getHistory: async (): Promise<ApiResponse<{ entries: HistoryEntry[]; count: number }>> => {
    try {
      const response = await apiClient.get("/v1/history");
      return { data: response.data };
    } catch (error) {
      if (axios.isAxiosError(error))
        return { error: true, message: error.response?.data?.message || error.message };
      return { error: true, message: "Unknown error occurred" };
    }
  },
};

// ─── System API ───────────────────────────────────────────────────────────────

export interface TaskInfo {
  name: string;
  status: "running" | "idle" | "error";
  last_run_at?: string;
  last_error?: string;
  run_count: number;
  next_run_hint?: string;
}

export const systemApi = {
  getTasks: async (): Promise<ApiResponse<{ tasks: TaskInfo[]; count: number }>> => {
    try {
      const response = await apiClient.get("/v1/system/tasks");
      return { data: response.data };
    } catch (error) {
      if (axios.isAxiosError(error))
        return { error: true, message: error.response?.data?.message || error.message };
      return { error: true, message: "Unknown error occurred" };
    }
  },
};
