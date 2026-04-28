package api

import (
	"testing"
	"time"
)

// ── NewTaskTracker ────────────────────────────────────────────────────────────

func TestNewTaskTracker_NotNil(t *testing.T) {
	tr := NewTaskTracker()
	if tr == nil {
		t.Fatal("NewTaskTracker() returned nil")
	}
}

func TestNewTaskTracker_Empty(t *testing.T) {
	tr := NewTaskTracker()
	if got := len(tr.GetAll()); got != 0 {
		t.Errorf("new tracker has %d tasks, want 0", got)
	}
}

// ── Register ──────────────────────────────────────────────────────────────────

func TestRegister_CreatesIdleTask(t *testing.T) {
	tr := NewTaskTracker()
	tr.Register("sync", "every 5 min")

	all := tr.GetAll()
	if len(all) != 1 {
		t.Fatalf("GetAll() len = %d, want 1", len(all))
	}
	task := all[0]
	if task.Name != "sync" {
		t.Errorf("Name = %q, want %q", task.Name, "sync")
	}
	if task.Status != TaskStatusIdle {
		t.Errorf("Status = %q, want %q", task.Status, TaskStatusIdle)
	}
	if task.NextRunHint != "every 5 min" {
		t.Errorf("NextRunHint = %q, want %q", task.NextRunHint, "every 5 min")
	}
	if task.RunCount != 0 {
		t.Errorf("RunCount = %d, want 0", task.RunCount)
	}
	if task.LastRunAt != nil {
		t.Errorf("LastRunAt should be nil for a newly registered task")
	}
}

func TestRegister_IsIdempotent(t *testing.T) {
	tr := NewTaskTracker()
	tr.Register("sync", "every 5 min")
	tr.Register("sync", "every 10 min") // second call must be ignored

	all := tr.GetAll()
	if len(all) != 1 {
		t.Fatalf("GetAll() len = %d, want 1 (duplicate register)", len(all))
	}
	// Original hint must be preserved.
	if all[0].NextRunHint != "every 5 min" {
		t.Errorf("NextRunHint = %q, want %q (second register must be no-op)", all[0].NextRunHint, "every 5 min")
	}
}

func TestRegister_MultipleDifferentTasks(t *testing.T) {
	tr := NewTaskTracker()
	tr.Register("task-a", "every 1 min")
	tr.Register("task-b", "every 2 min")
	tr.Register("task-c", "every 3 min")

	if got := len(tr.GetAll()); got != 3 {
		t.Errorf("GetAll() len = %d, want 3", got)
	}
}

// ── Start ─────────────────────────────────────────────────────────────────────

func TestStart_SetsRunningStatus(t *testing.T) {
	tr := NewTaskTracker()
	tr.Register("sync", "")
	before := time.Now()
	tr.Start("sync")

	task := findTask(t, tr, "sync")
	if task.Status != TaskStatusRunning {
		t.Errorf("Status = %q, want %q", task.Status, TaskStatusRunning)
	}
	if task.LastRunAt == nil {
		t.Fatal("LastRunAt is nil after Start()")
	}
	if task.LastRunAt.Before(before) {
		t.Errorf("LastRunAt (%v) is before test start time (%v)", task.LastRunAt, before)
	}
}

func TestStart_IncrementsRunCount(t *testing.T) {
	tr := NewTaskTracker()
	tr.Start("sync")
	tr.Start("sync")
	tr.Start("sync")

	task := findTask(t, tr, "sync")
	if task.RunCount != 3 {
		t.Errorf("RunCount = %d, want 3", task.RunCount)
	}
}

func TestStart_CreatesTaskIfNotRegistered(t *testing.T) {
	// getOrCreate is exercised via Start when the task was not registered.
	tr := NewTaskTracker()
	tr.Start("unregistered")

	task := findTask(t, tr, "unregistered")
	if task.Status != TaskStatusRunning {
		t.Errorf("Status = %q, want %q", task.Status, TaskStatusRunning)
	}
}

// ── Done ──────────────────────────────────────────────────────────────────────

func TestDone_SetsIdleAndClearsError(t *testing.T) {
	tr := NewTaskTracker()
	tr.Start("sync")
	tr.Fail("sync", "network error")
	tr.Done("sync")

	task := findTask(t, tr, "sync")
	if task.Status != TaskStatusIdle {
		t.Errorf("Status = %q, want %q", task.Status, TaskStatusIdle)
	}
	if task.LastError != "" {
		t.Errorf("LastError = %q, want empty (cleared by Done)", task.LastError)
	}
}

func TestDone_CreatesTaskIfNotRegistered(t *testing.T) {
	tr := NewTaskTracker()
	tr.Done("ghost")

	task := findTask(t, tr, "ghost")
	if task.Status != TaskStatusIdle {
		t.Errorf("Status = %q, want %q", task.Status, TaskStatusIdle)
	}
}

// ── Fail ──────────────────────────────────────────────────────────────────────

func TestFail_SetsErrorStatus(t *testing.T) {
	tr := NewTaskTracker()
	tr.Start("sync")
	tr.Fail("sync", "connection refused")

	task := findTask(t, tr, "sync")
	if task.Status != TaskStatusError {
		t.Errorf("Status = %q, want %q", task.Status, TaskStatusError)
	}
	if task.LastError != "connection refused" {
		t.Errorf("LastError = %q, want %q", task.LastError, "connection refused")
	}
}

func TestFail_CreatesTaskIfNotRegistered(t *testing.T) {
	tr := NewTaskTracker()
	tr.Fail("ghost", "some error")

	task := findTask(t, tr, "ghost")
	if task.Status != TaskStatusError {
		t.Errorf("Status = %q, want %q", task.Status, TaskStatusError)
	}
}

// ── GetAll ────────────────────────────────────────────────────────────────────

func TestGetAll_ReturnsSnapshot(t *testing.T) {
	tr := NewTaskTracker()
	tr.Register("a", "")
	tr.Register("b", "")

	all := tr.GetAll()
	// Mutate the snapshot — it must not affect the tracker.
	for i := range all {
		all[i].Status = TaskStatusError
	}

	// Internal state should be unchanged.
	for _, task := range tr.GetAll() {
		if task.Status != TaskStatusIdle {
			t.Errorf("task %q status polluted by snapshot mutation", task.Name)
		}
	}
}

// ── TaskStatus constants ──────────────────────────────────────────────────────

func TestTaskStatusConstants(t *testing.T) {
	if TaskStatusRunning != "running" {
		t.Errorf("TaskStatusRunning = %q, want %q", TaskStatusRunning, "running")
	}
	if TaskStatusIdle != "idle" {
		t.Errorf("TaskStatusIdle = %q, want %q", TaskStatusIdle, "idle")
	}
	if TaskStatusError != "error" {
		t.Errorf("TaskStatusError = %q, want %q", TaskStatusError, "error")
	}
}

// ── helpers ───────────────────────────────────────────────────────────────────

func findTask(t *testing.T, tr *TaskTracker, name string) TaskInfo {
	t.Helper()
	for _, task := range tr.GetAll() {
		if task.Name == name {
			return task
		}
	}
	t.Fatalf("task %q not found in tracker", name)
	return TaskInfo{}
}
