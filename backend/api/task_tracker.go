package api

import (
	"sync"
	"time"
)

// TaskStatus represents the state of a scheduled task.
type TaskStatus string

const (
	TaskStatusRunning  TaskStatus = "running"
	TaskStatusIdle     TaskStatus = "idle"
	TaskStatusError    TaskStatus = "error"
)

// TaskInfo holds the current state of a tracked background task.
type TaskInfo struct {
	Name        string     `json:"name"`
	Status      TaskStatus `json:"status"`
	LastRunAt   *time.Time `json:"last_run_at,omitempty"`
	LastError   string     `json:"last_error,omitempty"`
	RunCount    int        `json:"run_count"`
	NextRunHint string     `json:"next_run_hint,omitempty"` // human-readable hint, e.g. "every 5 min"
}

// TaskTracker tracks the state of background tasks in-memory.
type TaskTracker struct {
	mu    sync.RWMutex
	tasks map[string]*TaskInfo
}

// NewTaskTracker creates an empty TaskTracker.
func NewTaskTracker() *TaskTracker {
	return &TaskTracker{tasks: make(map[string]*TaskInfo)}
}

// Register registers a task by name with an optional schedule hint.
func (t *TaskTracker) Register(name, nextRunHint string) {
	t.mu.Lock()
	defer t.mu.Unlock()
	if _, exists := t.tasks[name]; !exists {
		t.tasks[name] = &TaskInfo{
			Name:        name,
			Status:      TaskStatusIdle,
			NextRunHint: nextRunHint,
		}
	}
}

// Start marks a task as running and records the start time.
func (t *TaskTracker) Start(name string) {
	t.mu.Lock()
	defer t.mu.Unlock()
	info := t.getOrCreate(name)
	now := time.Now()
	info.Status = TaskStatusRunning
	info.LastRunAt = &now
	info.RunCount++
}

// Done marks a task as idle (completed successfully).
func (t *TaskTracker) Done(name string) {
	t.mu.Lock()
	defer t.mu.Unlock()
	info := t.getOrCreate(name)
	info.Status = TaskStatusIdle
	info.LastError = ""
}

// Fail marks a task as errored with a message.
func (t *TaskTracker) Fail(name, errMsg string) {
	t.mu.Lock()
	defer t.mu.Unlock()
	info := t.getOrCreate(name)
	info.Status = TaskStatusError
	info.LastError = errMsg
}

// GetAll returns a snapshot of all tracked tasks.
func (t *TaskTracker) GetAll() []TaskInfo {
	t.mu.RLock()
	defer t.mu.RUnlock()
	out := make([]TaskInfo, 0, len(t.tasks))
	for _, v := range t.tasks {
		out = append(out, *v)
	}
	return out
}

// getOrCreate returns the TaskInfo for name, creating it if absent. Caller must hold t.mu.
func (t *TaskTracker) getOrCreate(name string) *TaskInfo {
	if info, ok := t.tasks[name]; ok {
		return info
	}
	info := &TaskInfo{Name: name, Status: TaskStatusIdle}
	t.tasks[name] = info
	return info
}
