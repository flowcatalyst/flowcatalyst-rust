// Command scheduled-jobs-runner mounts a scheduledjobs.Runner behind
// a stdlib HTTP handler. Each platform → SDK firing arrives as a JSON
// envelope on POST /webhooks/scheduled-job; the runner validates it,
// dispatches the matching handler in the background, and returns
// 202/400/404 immediately so the platform doesn't time out.
//
// Two handlers are registered:
//
//   - daily-rollup → tracksCompletion=true (assumed). Runs a fake
//     workload, logs progress back to the platform, returns a result
//     that becomes completion_result on the instance.
//   - heartbeat → tracksCompletion=false (assumed). Fire-and-forget;
//     handler runs but no completion call is made.
//
// Concurrency is enforced via lock.NewMemory() — fine for one-pod
// dev. In multi-pod prod, swap for lock/postgreslock or lock/redislock.
//
// # Run
//
//	FC_BASE_URL=http://localhost:7777  \
//	FC_TOKEN=eyJ...                    \
//	go run ./examples/scheduled-jobs-runner
//
// Then on the platform:
//
//	POST /api/scheduled-jobs
//	{ "code": "daily-rollup", "name": "...", "crons": ["0 0 * * *"],
//	  "tracksCompletion": true,
//	  "targetUrl": "http://localhost:8080/webhooks/scheduled-job" }
package main

import (
	"context"
	"encoding/json"
	"errors"
	"io"
	"log"
	"net/http"
	"os"
	"time"

	"github.com/flowcatalyst/flowcatalyst-rust/clients/go-sdk/client"
	"github.com/flowcatalyst/flowcatalyst-rust/clients/go-sdk/lock"
	"github.com/flowcatalyst/flowcatalyst-rust/clients/go-sdk/scheduledjobs"
)

func main() {
	c := client.New(
		mustEnv("FC_BASE_URL"),
		client.WithToken(mustEnv("FC_TOKEN")),
	)

	runner := scheduledjobs.NewBuilder(c, lock.NewMemory()).
		LockTTL(15 * time.Minute).
		OnError(func(err error, env *scheduledjobs.Envelope) {
			// Hook fires on log/complete callback failures, lock
			// failures, and handler panics. Treat as warnings; the
			// runner has already done what it can.
			log.Printf("runner: code=%s instance=%s: %v", env.JobCode, env.InstanceID, err)
		}).
		Handler("daily-rollup", dailyRollup).
		Handler("heartbeat", heartbeat).
		Build()

	mux := http.NewServeMux()
	mux.HandleFunc("POST /webhooks/scheduled-job", handleWebhook(runner))

	srv := &http.Server{Addr: ":8080", Handler: mux}
	log.Println("scheduled-jobs runner listening on :8080")
	if err := srv.ListenAndServe(); err != nil && !errors.Is(err, http.ErrServerClosed) {
		log.Fatal(err)
	}
}

func handleWebhook(r *scheduledjobs.Runner) http.HandlerFunc {
	return func(w http.ResponseWriter, req *http.Request) {
		raw, err := io.ReadAll(req.Body)
		if err != nil {
			http.Error(w, `{"error":"read body"}`, http.StatusBadRequest)
			return
		}
		switch res := r.Process(raw); res.Kind {
		case scheduledjobs.ResultAccepted:
			w.WriteHeader(http.StatusAccepted)
			_, _ = w.Write([]byte(`{"ok":true}`))
		case scheduledjobs.ResultNotFound:
			http.Error(w, jsonErr(res.Message), http.StatusNotFound)
		case scheduledjobs.ResultBadRequest:
			http.Error(w, jsonErr(res.Message), http.StatusBadRequest)
		}
	}
}

// ───────────────────────────────────────────────────────────────────
// Handlers
// ───────────────────────────────────────────────────────────────────

func dailyRollup(ctx context.Context, hctx *scheduledjobs.HandlerContext) (json.RawMessage, error) {
	hctx.Log(ctx, "starting daily rollup", nil)
	defer hctx.Log(ctx, "rollup finished", nil)

	// Do work … honour ctx for cancellation if the runner set a timeout
	// from envelope.TimeoutSeconds.
	select {
	case <-time.After(50 * time.Millisecond):
	case <-ctx.Done():
		return nil, ctx.Err()
	}
	return json.RawMessage(`{"rolledUp":42}`), nil
}

func heartbeat(_ context.Context, hctx *scheduledjobs.HandlerContext) (json.RawMessage, error) {
	hctx.Log(context.Background(), "alive", &scheduledjobs.LogOptions{Level: "DEBUG"})
	return nil, nil
}

// ───────────────────────────────────────────────────────────────────
// Helpers
// ───────────────────────────────────────────────────────────────────

func jsonErr(msg string) string {
	b, _ := json.Marshal(map[string]string{"error": msg})
	return string(b)
}

func mustEnv(k string) string {
	v := os.Getenv(k)
	if v == "" {
		log.Fatalf("%s is required", k)
	}
	return v
}
