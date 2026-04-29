/**
 * Unit tests for src/lib/api/index.ts
 *
 * All HTTP calls are intercepted via vi.mock('axios') — no network traffic
 * ever leaves this process.
 */
import { vi, describe, it, expect, beforeEach } from "vitest";

// ── Shared mock functions ─────────────────────────────────────────────────────
// vi.hoisted() ensures these are created before vi.mock() executes, which
// itself is hoisted before any import statements by Vitest.
const { mockGet, mockPost, mockPut, mockDelete, mockIsAxiosError } = vi.hoisted(() => ({
    mockGet: vi.fn(),
    mockPost: vi.fn(),
    mockPut: vi.fn(),
    mockDelete: vi.fn(),
    mockIsAxiosError: vi.fn(),
}));

vi.mock("axios", () => ({
    default: {
        create: () => ({
            get: mockGet,
            post: mockPost,
            put: mockPut,
            delete: mockDelete,
            interceptors: {
                request: { use: vi.fn() },
                response: { use: vi.fn() },
            },
        }),
        isAxiosError: mockIsAxiosError,
    },
}));

// Import after mocks are set up.
import {
    userApi,
    movieApi,
    tvApi,
    wantedApi,
    libraryApi,
    indexerApi,
    historyApi,
    systemApi,
} from "$lib/api/index";

// ── helpers ───────────────────────────────────────────────────────────────────

/** Simulate a successful axios response. */
function resolveWith(data: unknown) {
    return Promise.resolve({ data });
}

/** Build an AxiosError-like object that passes isAxiosError(). */
function axiosError(message: string, responseMessage?: string) {
    const err = {
        _isAxiosError: true,
        message,
        response: responseMessage ? { data: { message: responseMessage } } : undefined,
    };
    return err;
}

beforeEach(() => {
    vi.clearAllMocks();
    // Default: isAxiosError returns true for anything with _isAxiosError flag.
    mockIsAxiosError.mockImplementation(
        (e: unknown) => !!(e as Record<string, unknown>)?._isAxiosError,
    );
});

// ── userApi ───────────────────────────────────────────────────────────────────

describe("userApi.getUsers", () => {
    it("returns data on success", async () => {
        const users = [{ id: "1", username: "alice", email: "a@test.com" }];
        mockGet.mockResolvedValueOnce(resolveWith({ users }));

        const result = await userApi.getUsers();
        expect(result.data).toEqual(users);
        expect(result.error).toBeUndefined();
    });

    it("returns AxiosError message from response body", async () => {
        mockGet.mockRejectedValueOnce(axiosError("Request failed", "Unauthorized"));

        const result = await userApi.getUsers();
        expect(result.error).toBe(true);
        expect(result.message).toBe("Unauthorized");
    });

    it("falls back to error.message when response body has no message", async () => {
        mockGet.mockRejectedValueOnce(axiosError("Network Error"));

        const result = await userApi.getUsers();
        expect(result.error).toBe(true);
        expect(result.message).toBe("Network Error");
    });

    it("returns generic message for non-Axios errors", async () => {
        mockIsAxiosError.mockReturnValue(false);
        mockGet.mockRejectedValueOnce(new Error("unexpected"));

        const result = await userApi.getUsers();
        expect(result.error).toBe(true);
        expect(result.message).toBe("Unknown error occurred");
    });
});

describe("userApi.getUser", () => {
    it("returns data on success", async () => {
        const user = { id: "42", username: "bob", email: "b@test.com" };
        mockGet.mockResolvedValueOnce(resolveWith(user));

        const result = await userApi.getUser("42");
        expect(result.data).toEqual(user);
        expect(mockGet).toHaveBeenCalledWith("/v1/users/42");
    });

    it("returns error on failure", async () => {
        mockGet.mockRejectedValueOnce(axiosError("Not Found", "User not found"));

        const result = await userApi.getUser("99");
        expect(result.error).toBe(true);
        expect(result.message).toBe("User not found");
    });
});

describe("userApi.createUser", () => {
    it("returns created user on success", async () => {
        const user = { id: "1", username: "new", email: "new@test.com" };
        mockPost.mockResolvedValueOnce(resolveWith(user));

        const result = await userApi.createUser({ username: "new", email: "new@test.com" });
        expect(result.data).toEqual(user);
    });

    it("returns error on failure", async () => {
        mockPost.mockRejectedValueOnce(axiosError("Conflict", "Username taken"));

        const result = await userApi.createUser({ username: "taken", email: "x@x.com" });
        expect(result.error).toBe(true);
        expect(result.message).toBe("Username taken");
    });
});

describe("userApi.updateUser", () => {
    it("returns updated user on success", async () => {
        const user = { id: "1", username: "updated", email: "u@test.com" };
        mockPut.mockResolvedValueOnce(resolveWith(user));

        const result = await userApi.updateUser("1", { username: "updated" });
        expect(result.data).toEqual(user);
        expect(mockPut).toHaveBeenCalledWith("/v1/users/1", { username: "updated" });
    });

    it("returns error on failure", async () => {
        mockPut.mockRejectedValueOnce(axiosError("Bad Request", "Invalid email"));

        const result = await userApi.updateUser("1", { email: "bad" });
        expect(result.error).toBe(true);
    });
});

describe("userApi.deleteUser", () => {
    it("returns null data on success", async () => {
        mockDelete.mockResolvedValueOnce(resolveWith({}));

        const result = await userApi.deleteUser("1");
        expect(result.data).toBeNull();
        expect(mockDelete).toHaveBeenCalledWith("/v1/users/1");
    });

    it("returns error on failure", async () => {
        mockDelete.mockRejectedValueOnce(axiosError("Not Found", "User not found"));

        const result = await userApi.deleteUser("99");
        expect(result.error).toBe(true);
        expect(result.message).toBe("User not found");
    });
});

// ── movieApi ──────────────────────────────────────────────────────────────────

describe("movieApi.getMovies", () => {
    it("returns movies on success", async () => {
        mockGet.mockResolvedValueOnce(resolveWith({ movies: [], count: 0 }));
        const result = await movieApi.getMovies();
        expect(result.data).toEqual({ movies: [], count: 0 });
    });

    it("returns error on failure", async () => {
        mockGet.mockRejectedValueOnce(axiosError("Server error", "Internal error"));
        const result = await movieApi.getMovies();
        expect(result.error).toBe(true);
        expect(result.message).toBe("Internal error");
    });
});

describe("movieApi.searchMovies", () => {
    it("calls correct endpoint with query param", async () => {
        mockGet.mockResolvedValueOnce(resolveWith({ movies: [], count: 0 }));
        await movieApi.searchMovies("inception");
        expect(mockGet).toHaveBeenCalledWith("/v1/movies/search", { params: { q: "inception" } });
    });

    it("returns error on failure", async () => {
        mockGet.mockRejectedValueOnce(axiosError("err"));
        const result = await movieApi.searchMovies("x");
        expect(result.error).toBe(true);
    });
});

describe("movieApi.getMovie", () => {
    it("returns movie on success", async () => {
        const movie = { id: "10", title: "Inception" };
        mockGet.mockResolvedValueOnce(resolveWith(movie));
        const result = await movieApi.getMovie("10");
        expect(result.data).toEqual(movie);
    });

    it("returns error on failure", async () => {
        mockGet.mockRejectedValueOnce(axiosError("err", "Not found"));
        const result = await movieApi.getMovie("999");
        expect(result.error).toBe(true);
    });
});

describe("movieApi.requestMovie", () => {
    it("posts request and returns response data", async () => {
        mockPost.mockResolvedValueOnce(resolveWith({ success: true, message: "ok", id: "w1" }));
        const result = await movieApi.requestMovie("10", { min_quality: "1080p" });
        expect(result.data).toEqual({ success: true, message: "ok", id: "w1" });
        expect(mockPost).toHaveBeenCalledWith("/v1/movies/10/request", { min_quality: "1080p" });
    });

    it("passes empty object when no options provided", async () => {
        mockPost.mockResolvedValueOnce(resolveWith({ success: true, message: "ok", id: "w2" }));
        await movieApi.requestMovie("10");
        expect(mockPost).toHaveBeenCalledWith("/v1/movies/10/request", {});
    });

    it("returns error on failure", async () => {
        mockPost.mockRejectedValueOnce(axiosError("err", "Already exists"));
        const result = await movieApi.requestMovie("10");
        expect(result.error).toBe(true);
    });
});

describe("movieApi.fetchPopular", () => {
    it("returns null data on success", async () => {
        mockPost.mockResolvedValueOnce(resolveWith({}));
        const result = await movieApi.fetchPopular();
        expect(result.data).toBeNull();
    });

    it("returns error on failure", async () => {
        mockPost.mockRejectedValueOnce(axiosError("err"));
        const result = await movieApi.fetchPopular();
        expect(result.error).toBe(true);
    });
});

describe("movieApi.fetchNowPlaying", () => {
    it("returns null data on success", async () => {
        mockPost.mockResolvedValueOnce(resolveWith({}));
        const result = await movieApi.fetchNowPlaying();
        expect(result.data).toBeNull();
    });

    it("returns error on failure", async () => {
        mockPost.mockRejectedValueOnce(axiosError("err"));
        const result = await movieApi.fetchNowPlaying();
        expect(result.error).toBe(true);
    });
});

// ── tvApi ─────────────────────────────────────────────────────────────────────

describe("tvApi.getTVShows", () => {
    it("returns tvshows on success", async () => {
        mockGet.mockResolvedValueOnce(resolveWith({ tvshows: [], count: 0 }));
        const result = await tvApi.getTVShows();
        expect(result.data).toEqual({ tvshows: [], count: 0 });
    });

    it("returns error on failure", async () => {
        mockGet.mockRejectedValueOnce(axiosError("err", "DB error"));
        const result = await tvApi.getTVShows();
        expect(result.error).toBe(true);
    });
});

describe("tvApi.searchTVShows", () => {
    it("calls correct endpoint", async () => {
        mockGet.mockResolvedValueOnce(resolveWith({ tvshows: [], count: 0 }));
        await tvApi.searchTVShows("breaking bad");
        expect(mockGet).toHaveBeenCalledWith("/v1/tvshows/search", {
            params: { q: "breaking bad" },
        });
    });

    it("returns error on failure", async () => {
        mockGet.mockRejectedValueOnce(axiosError("err"));
        const result = await tvApi.searchTVShows("x");
        expect(result.error).toBe(true);
    });
});

describe("tvApi.getTVShow", () => {
    it("returns show on success", async () => {
        mockGet.mockResolvedValueOnce(resolveWith({ id: "1", name: "Breaking Bad" }));
        const result = await tvApi.getTVShow("1");
        expect((result.data as Record<string, unknown>)?.name).toBe("Breaking Bad");
    });

    it("returns error on failure", async () => {
        mockGet.mockRejectedValueOnce(axiosError("err", "Not found"));
        const result = await tvApi.getTVShow("999");
        expect(result.error).toBe(true);
    });
});

describe("tvApi.requestTVShow", () => {
    it("posts and returns response data", async () => {
        mockPost.mockResolvedValueOnce(resolveWith({ success: true, message: "ok", id: "w1" }));
        const result = await tvApi.requestTVShow("1", { season: 1 });
        expect(result.data).toEqual({ success: true, message: "ok", id: "w1" });
    });

    it("passes empty object when no options provided", async () => {
        mockPost.mockResolvedValueOnce(resolveWith({ success: true, message: "ok", id: "w2" }));
        await tvApi.requestTVShow("1");
        expect(mockPost).toHaveBeenCalledWith("/v1/tvshows/1/request", {});
    });

    it("returns error on failure", async () => {
        mockPost.mockRejectedValueOnce(axiosError("err"));
        const result = await tvApi.requestTVShow("1");
        expect(result.error).toBe(true);
    });
});

describe("tvApi.fetchPopularTV", () => {
    it("returns null data on success", async () => {
        mockPost.mockResolvedValueOnce(resolveWith({}));
        const result = await tvApi.fetchPopularTV();
        expect(result.data).toBeNull();
    });

    it("returns error on failure", async () => {
        mockPost.mockRejectedValueOnce(axiosError("err"));
        const result = await tvApi.fetchPopularTV();
        expect(result.error).toBe(true);
    });
});

describe("tvApi.getEpisodes", () => {
    it("returns episodes on success", async () => {
        const episodes = [
            {
                id: "e1",
                show_id: "1",
                season: 1,
                number: 1,
                title: "Pilot",
                air_date: "",
                overview: "",
                status: "downloaded",
            },
        ];
        const payload = { show_id: "1", episodes, count: 1 };
        mockGet.mockResolvedValueOnce(resolveWith(payload));
        const result = await tvApi.getEpisodes("1");
        expect(result.data).toEqual(payload);
    });

    it("returns error on failure", async () => {
        mockGet.mockRejectedValueOnce(axiosError("err", "Show not found"));
        const result = await tvApi.getEpisodes("999");
        expect(result.error).toBe(true);
    });
});

// ── wantedApi ─────────────────────────────────────────────────────────────────

describe("wantedApi.getWanted", () => {
    it("returns wanted items on success", async () => {
        mockGet.mockResolvedValueOnce(resolveWith({ items: [], count: 0 }));
        const result = await wantedApi.getWanted();
        expect(result.data).toEqual({ items: [], count: 0 });
    });

    it("returns error on failure", async () => {
        mockGet.mockRejectedValueOnce(axiosError("err", "Server error"));
        const result = await wantedApi.getWanted();
        expect(result.error).toBe(true);
    });
});

describe("wantedApi.deleteWanted", () => {
    it("returns null data on success", async () => {
        mockDelete.mockResolvedValueOnce(resolveWith({}));
        const result = await wantedApi.deleteWanted("w1");
        expect(result.data).toBeNull();
        expect(mockDelete).toHaveBeenCalledWith("/v1/wanted/w1");
    });

    it("returns error on failure", async () => {
        mockDelete.mockRejectedValueOnce(axiosError("err", "Not found"));
        const result = await wantedApi.deleteWanted("missing");
        expect(result.error).toBe(true);
    });
});

// ── libraryApi ────────────────────────────────────────────────────────────────

describe("libraryApi.scan", () => {
    it("returns scan results on success", async () => {
        const scanResult = {
            movies: { matched: [], unmatched: [] },
            tvshows: { matched: [], unmatched: [] },
            scanned_at: "2024-01-01T00:00:00Z",
        };
        mockGet.mockResolvedValueOnce(resolveWith(scanResult));
        const result = await libraryApi.scan();
        expect(result.data).toEqual(scanResult);
        expect(mockGet).toHaveBeenCalledWith("/v1/library/scan");
    });

    it("returns error on failure", async () => {
        mockGet.mockRejectedValueOnce(axiosError("err", "Scan failed"));
        const result = await libraryApi.scan();
        expect(result.error).toBe(true);
    });
});

// ── indexerApi ────────────────────────────────────────────────────────────────

describe("indexerApi.getHealth", () => {
    it("returns indexer health on success", async () => {
        const health = {
            indexers: [
                {
                    name: "YTS",
                    enabled: true,
                    last_success: "",
                    last_result_count: 10,
                    total_searches: 5,
                    total_errors: 0,
                    error_rate: 0,
                    avg_response_ms: 120,
                },
            ],
        };
        mockGet.mockResolvedValueOnce(resolveWith(health));
        const result = await indexerApi.getHealth();
        expect(result.data).toEqual(health);
        expect(mockGet).toHaveBeenCalledWith("/v1/indexers/health");
    });

    it("returns error on failure", async () => {
        mockGet.mockRejectedValueOnce(axiosError("err", "Registry unavailable"));
        const result = await indexerApi.getHealth();
        expect(result.error).toBe(true);
    });
});

// ── historyApi ────────────────────────────────────────────────────────────────

describe("historyApi.getHistory", () => {
    it("returns history entries on success", async () => {
        const history = { entries: [], count: 0 };
        mockGet.mockResolvedValueOnce(resolveWith(history));
        const result = await historyApi.getHistory();
        expect(result.data).toEqual(history);
        expect(mockGet).toHaveBeenCalledWith("/v1/history");
    });

    it("returns error on failure", async () => {
        mockGet.mockRejectedValueOnce(axiosError("err", "History error"));
        const result = await historyApi.getHistory();
        expect(result.error).toBe(true);
    });
});

// ── systemApi ─────────────────────────────────────────────────────────────────

describe("systemApi.getTasks", () => {
    it("returns tasks on success", async () => {
        const tasks = {
            tasks: [{ name: "wanted-poller", status: "idle" as const, run_count: 5 }],
            count: 1,
        };
        mockGet.mockResolvedValueOnce(resolveWith(tasks));
        const result = await systemApi.getTasks();
        expect(result.data).toEqual(tasks);
        expect(mockGet).toHaveBeenCalledWith("/v1/system/tasks");
    });

    it("returns error on failure", async () => {
        mockGet.mockRejectedValueOnce(axiosError("err", "System error"));
        const result = await systemApi.getTasks();
        expect(result.error).toBe(true);
    });
});

// ── non-Axios unknown errors ──────────────────────────────────────────────────

describe("non-Axios error fallback", () => {
    it("movieApi.getMovies returns unknown error message", async () => {
        mockIsAxiosError.mockReturnValue(false);
        mockGet.mockRejectedValueOnce("a plain string error");
        const result = await movieApi.getMovies();
        expect(result.error).toBe(true);
        expect(result.message).toBe("Unknown error occurred");
    });

    it("tvApi.getTVShows returns unknown error message", async () => {
        mockIsAxiosError.mockReturnValue(false);
        mockGet.mockRejectedValueOnce(new TypeError("something"));
        const result = await tvApi.getTVShows();
        expect(result.error).toBe(true);
        expect(result.message).toBe("Unknown error occurred");
    });

    it("historyApi.getHistory returns unknown error message", async () => {
        mockIsAxiosError.mockReturnValue(false);
        mockGet.mockRejectedValueOnce(new Error("unexpected"));
        const result = await historyApi.getHistory();
        expect(result.error).toBe(true);
        expect(result.message).toBe("Unknown error occurred");
    });

    it("systemApi.getTasks returns unknown error message", async () => {
        mockIsAxiosError.mockReturnValue(false);
        mockGet.mockRejectedValueOnce(new Error("unexpected"));
        const result = await systemApi.getTasks();
        expect(result.error).toBe(true);
        expect(result.message).toBe("Unknown error occurred");
    });

    it("wantedApi.deleteWanted returns unknown error message", async () => {
        mockIsAxiosError.mockReturnValue(false);
        mockDelete.mockRejectedValueOnce(new Error("unexpected"));
        const result = await wantedApi.deleteWanted("w1");
        expect(result.error).toBe(true);
        expect(result.message).toBe("Unknown error occurred");
    });

    it("libraryApi.scan returns unknown error message", async () => {
        mockIsAxiosError.mockReturnValue(false);
        mockGet.mockRejectedValueOnce(new Error("unexpected"));
        const result = await libraryApi.scan();
        expect(result.error).toBe(true);
        expect(result.message).toBe("Unknown error occurred");
    });

    it("indexerApi.getHealth returns unknown error message", async () => {
        mockIsAxiosError.mockReturnValue(false);
        mockGet.mockRejectedValueOnce(new Error("unexpected"));
        const result = await indexerApi.getHealth();
        expect(result.error).toBe(true);
        expect(result.message).toBe("Unknown error occurred");
    });
});

// ── error.message fallback (no response body) ─────────────────────────────────
// For each API function that was only tested with a response-body error message,
// add a test that drives the `|| error.message` branch.

describe("error.message fallback (no response body)", () => {
    it("userApi.getUser falls back to error.message", async () => {
        mockGet.mockRejectedValueOnce(axiosError("raw-msg"));
        const result = await userApi.getUser("1");
        expect(result.error).toBe(true);
        expect(result.message).toBe("raw-msg");
    });

    it("userApi.createUser falls back to error.message", async () => {
        mockPost.mockRejectedValueOnce(axiosError("raw-msg"));
        const result = await userApi.createUser({ username: "x", email: "x@x.com" });
        expect(result.error).toBe(true);
        expect(result.message).toBe("raw-msg");
    });

    it("userApi.updateUser falls back to error.message", async () => {
        mockPut.mockRejectedValueOnce(axiosError("raw-msg"));
        const result = await userApi.updateUser("1", { username: "x" });
        expect(result.error).toBe(true);
        expect(result.message).toBe("raw-msg");
    });

    it("userApi.deleteUser falls back to error.message", async () => {
        mockDelete.mockRejectedValueOnce(axiosError("raw-msg"));
        const result = await userApi.deleteUser("1");
        expect(result.error).toBe(true);
        expect(result.message).toBe("raw-msg");
    });

    it("movieApi.getMovies falls back to error.message", async () => {
        mockGet.mockRejectedValueOnce(axiosError("raw-msg"));
        const result = await movieApi.getMovies();
        expect(result.error).toBe(true);
        expect(result.message).toBe("raw-msg");
    });

    it("movieApi.getMovie falls back to error.message", async () => {
        mockGet.mockRejectedValueOnce(axiosError("raw-msg"));
        const result = await movieApi.getMovie("1");
        expect(result.error).toBe(true);
        expect(result.message).toBe("raw-msg");
    });

    it("movieApi.requestMovie falls back to error.message", async () => {
        mockPost.mockRejectedValueOnce(axiosError("raw-msg"));
        const result = await movieApi.requestMovie("1");
        expect(result.error).toBe(true);
        expect(result.message).toBe("raw-msg");
    });

    it("tvApi.getTVShows falls back to error.message", async () => {
        mockGet.mockRejectedValueOnce(axiosError("raw-msg"));
        const result = await tvApi.getTVShows();
        expect(result.error).toBe(true);
        expect(result.message).toBe("raw-msg");
    });

    it("tvApi.getTVShow falls back to error.message", async () => {
        mockGet.mockRejectedValueOnce(axiosError("raw-msg"));
        const result = await tvApi.getTVShow("1");
        expect(result.error).toBe(true);
        expect(result.message).toBe("raw-msg");
    });

    it("tvApi.getEpisodes falls back to error.message", async () => {
        mockGet.mockRejectedValueOnce(axiosError("raw-msg"));
        const result = await tvApi.getEpisodes("1");
        expect(result.error).toBe(true);
        expect(result.message).toBe("raw-msg");
    });

    it("wantedApi.getWanted falls back to error.message", async () => {
        mockGet.mockRejectedValueOnce(axiosError("raw-msg"));
        const result = await wantedApi.getWanted();
        expect(result.error).toBe(true);
        expect(result.message).toBe("raw-msg");
    });

    it("wantedApi.deleteWanted falls back to error.message", async () => {
        mockDelete.mockRejectedValueOnce(axiosError("raw-msg"));
        const result = await wantedApi.deleteWanted("w1");
        expect(result.error).toBe(true);
        expect(result.message).toBe("raw-msg");
    });

    it("libraryApi.scan falls back to error.message", async () => {
        mockGet.mockRejectedValueOnce(axiosError("raw-msg"));
        const result = await libraryApi.scan();
        expect(result.error).toBe(true);
        expect(result.message).toBe("raw-msg");
    });

    it("indexerApi.getHealth falls back to error.message", async () => {
        mockGet.mockRejectedValueOnce(axiosError("raw-msg"));
        const result = await indexerApi.getHealth();
        expect(result.error).toBe(true);
        expect(result.message).toBe("raw-msg");
    });

    it("historyApi.getHistory falls back to error.message", async () => {
        mockGet.mockRejectedValueOnce(axiosError("raw-msg"));
        const result = await historyApi.getHistory();
        expect(result.error).toBe(true);
        expect(result.message).toBe("raw-msg");
    });

    it("systemApi.getTasks falls back to error.message", async () => {
        mockGet.mockRejectedValueOnce(axiosError("raw-msg"));
        const result = await systemApi.getTasks();
        expect(result.error).toBe(true);
        expect(result.message).toBe("raw-msg");
    });
});
