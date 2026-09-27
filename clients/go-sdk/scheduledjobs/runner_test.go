package scheduledjobs_test

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"

	"github.com/flowcatalyst/flowcatalyst-rust/clients/go-sdk/client"
	"github.com/flowcatalyst/flowcatalyst-rust/clients/go-sdk/lock"
	"github.com/flowcatalyst/flowcatalyst-rust/clients/go-sdk/scheduledjobs"
)

// platformMock records inbound calls to /log and /complete so tests
// can assert on the runner's side-effects.
type platformMock struct {
	mu       sync.Mutex
	logs     []logEntry
	complete *completeEntry
}

type logEntry struct {
	InstanceID string
	Body       client.InstanceLogRequest
}

type completeEntry struct {
	InstanceID string
	Body       client.InstanceCompleteRequest
}

func newPlatform(t *testing.T) (*httptest.Server, *platformMock) {
	t.Helper()
	pm := &platformMock{}
	mux := http.NewServeMux()
	mux.HandleFunc("/api/scheduled-jobs/instances/", func(w http.ResponseWriter, r *http.Request) {
		// path = /api/scheduled-jobs/instances/{id}/(log|complete)
		segs := splitPath(r.URL.Path)
		require.GreaterOrEqual(t, len(segs), 6, "path: %s", r.URL.Path)
		instanceID := segs[4]
		action := segs[5]
		switch action {
		case "log":
			var body client.InstanceLogRequest
			require.NoError(t, json.NewDecoder(r.Body).Decode(&body))
			pm.mu.Lock()
			pm.logs = append(pm.logs, logEntry{InstanceID: instanceID, Body: body})
			pm.mu.Unlock()
			w.WriteHeader(http.StatusOK)
			_, _ = w.Write([]byte(`{"id":"lg_1","instanceId":"` + instanceID + `","level":"INFO","message":"","createdAt":""}`))
		case "complete":
			var body client.InstanceCompleteRequest
			require.NoError(t, json.NewDecoder(r.Body).Decode(&body))
			pm.mu.Lock()
			pm.complete = &completeEntry{InstanceID: instanceID, Body: body}
			pm.mu.Unlock()
			w.WriteHeader(http.StatusOK)
			_, _ = w.Write([]byte(`{"id":"` + instanceID + `","scheduledJobId":"sjb_1","jobCode":"x","triggerKind":"CRON","firedAt":"","status":"COMPLETED","deliveryAttempts":1,"createdAt":""}`))
		default:
			http.NotFound(w, r)
		}
	})
	srv := httptest.NewServer(mux)
	t.Cleanup(srv.Close)
	return srv, pm
}

func splitPath(p string) []string {
	out := []string{}
	cur := ""
	for _, c := range p {
		if c == '/' {
			out = append(out, cur)
			cur = ""
		} else {
			cur += string(c)
		}
	}
	out = append(out, cur)
	return out
}

func makeEnvelope(jobCode, instanceID string, tracksCompletion bool) json.RawMessage {
	env := scheduledjobs.Envelope{
		JobID:            "sjb_1",
		JobCode:          jobCode,
		InstanceID:       instanceID,
		FiredAt:          "2026-01-01T00:00:00Z",
		TriggerKind:      scheduledjobs.TriggerCron,
		TracksCompletion: tracksCompletion,
	}
	raw, _ := json.Marshal(env)
	return raw
}

func TestProcessDispatchesHandlerAndReportsSuccess(t *testing.T) {
	srv, pm := newPlatform(t)
	api := client.New(srv.URL)

	var ran atomic.Bool
	r := scheduledjobs.NewBuilder(api, lock.NewNoOp()).
		Handler("daily", func(ctx context.Context, hctx *scheduledjobs.HandlerContext) (json.RawMessage, error) {
			ran.Store(true)
			hctx.Log(ctx, "starting", nil)
			return json.RawMessage(`{"processed":42}`), nil
		}).
		Build()

	res := r.Process(makeEnvelope("daily", "ins_1", true))
	assert.Equal(t, scheduledjobs.ResultAccepted, res.Kind)

	r.Wait()
	assert.True(t, ran.Load())
	require.NotNil(t, pm.complete)
	assert.Equal(t, "ins_1", pm.complete.InstanceID)
	assert.Equal(t, client.CompletionStatusSuccess, pm.complete.Body.Status)
	assert.JSONEq(t, `{"processed":42}`, string(pm.complete.Body.Result))
	require.Len(t, pm.logs, 1)
	assert.Equal(t, "starting", pm.logs[0].Body.Message)
}

func TestProcessReportsHandlerFailure(t *testing.T) {
	srv, pm := newPlatform(t)
	api := client.New(srv.URL)

	r := scheduledjobs.NewBuilder(api, lock.NewNoOp()).
		Handler("broken", func(_ context.Context, _ *scheduledjobs.HandlerContext) (json.RawMessage, error) {
			return nil, assertErr("boom")
		}).
		Build()

	res := r.Process(makeEnvelope("broken", "ins_2", true))
	require.Equal(t, scheduledjobs.ResultAccepted, res.Kind)

	r.Wait()
	require.NotNil(t, pm.complete)
	assert.Equal(t, client.CompletionStatusFailure, pm.complete.Body.Status)
	assert.Contains(t, string(pm.complete.Body.Result), "boom")
}

func TestProcessUnknownJobCodeIsNotFound(t *testing.T) {
	srv, _ := newPlatform(t)
	api := client.New(srv.URL)

	r := scheduledjobs.NewBuilder(api, lock.NewNoOp()).Build()
	res := r.Process(makeEnvelope("unknown", "ins_x", true))
	assert.Equal(t, scheduledjobs.ResultNotFound, res.Kind)
	assert.Contains(t, res.Message, "unknown")
}

func TestProcessMalformedEnvelopeIsBadRequest(t *testing.T) {
	srv, _ := newPlatform(t)
	api := client.New(srv.URL)

	r := scheduledjobs.NewBuilder(api, lock.NewNoOp()).Build()
	res := r.Process(json.RawMessage(`{"jobCode":"x","triggerKind":"GARBAGE","firedAt":"","jobId":"","instanceId":""}`))
	assert.Equal(t, scheduledjobs.ResultBadRequest, res.Kind)
}

func TestLockContentionReportsLockHeldFailure(t *testing.T) {
	srv, pm := newPlatform(t)
	api := client.New(srv.URL)

	// Memory lock pre-held by an out-of-band caller so the runner finds
	// the key contended on Acquire.
	lp := lock.NewMemory()
	held, err := lp.Acquire(context.Background(), "scheduled-job:single", time.Minute)
	require.NoError(t, err)
	require.NotNil(t, held)
	t.Cleanup(func() { _ = held.Release(context.Background()) })

	var ran atomic.Bool
	r := scheduledjobs.NewBuilder(api, lp).
		Handler("single", func(_ context.Context, _ *scheduledjobs.HandlerContext) (json.RawMessage, error) {
			ran.Store(true)
			return nil, nil
		}).
		Build()

	res := r.Process(makeEnvelope("single", "ins_3", true))
	require.Equal(t, scheduledjobs.ResultAccepted, res.Kind)

	r.Wait()
	assert.False(t, ran.Load(), "handler must not run while lock is contended")
	require.NotNil(t, pm.complete)
	assert.Equal(t, client.CompletionStatusFailure, pm.complete.Body.Status)
	assert.Contains(t, string(pm.complete.Body.Result), "lock-held")
}

func TestNoCompletionCallbackWhenTracksCompletionFalse(t *testing.T) {
	srv, pm := newPlatform(t)
	api := client.New(srv.URL)

	r := scheduledjobs.NewBuilder(api, lock.NewNoOp()).
		Handler("fire-and-forget", func(_ context.Context, _ *scheduledjobs.HandlerContext) (json.RawMessage, error) {
			return nil, nil
		}).
		Build()

	res := r.Process(makeEnvelope("fire-and-forget", "ins_4", false))
	require.Equal(t, scheduledjobs.ResultAccepted, res.Kind)
	r.Wait()
	assert.Nil(t, pm.complete, "no complete callback expected when tracksCompletion=false")
}

func TestHandlerPanicSurfacedViaOnError(t *testing.T) {
	srv, pm := newPlatform(t)
	api := client.New(srv.URL)

	var captured *scheduledjobs.RunnerError
	var mu sync.Mutex
	r := scheduledjobs.NewBuilder(api, lock.NewNoOp()).
		OnError(func(err error, _ *scheduledjobs.Envelope) {
			mu.Lock()
			defer mu.Unlock()
			if re, ok := err.(*scheduledjobs.RunnerError); ok {
				captured = re
			}
		}).
		Handler("panicky", func(_ context.Context, _ *scheduledjobs.HandlerContext) (json.RawMessage, error) {
			panic("kaboom")
		}).
		Build()

	res := r.Process(makeEnvelope("panicky", "ins_5", true))
	require.Equal(t, scheduledjobs.ResultAccepted, res.Kind)
	r.Wait()

	mu.Lock()
	defer mu.Unlock()
	require.NotNil(t, captured, "OnError must fire on panic")
	assert.Equal(t, scheduledjobs.ErrHandlerPanicked, captured.Kind)
	require.NotNil(t, pm.complete)
	assert.Equal(t, client.CompletionStatusFailure, pm.complete.Body.Status)
	assert.Contains(t, string(pm.complete.Body.Result), "kaboom")
}

// assertErr is a tiny error builder used inside tests.
type assertErr string

func (a assertErr) Error() string { return string(a) }
