// Command webhook-receiver shows how to mount the webhook validator
// behind a stdlib HTTP handler. Each inbound POST is validated against
// the HMAC-SHA256 signature header before the body is parsed; failures
// map to 401 / 403 with the matching sentinel.
//
// # Run
//
//	FLOWCATALYST_SIGNING_SECRET=shh go run ./examples/webhook-receiver
//
// In another terminal:
//
//	ts=$(date +%s)
//	body='{"hello":"world"}'
//	sig=$(echo -n "${ts}${body}" | openssl dgst -sha256 -hmac shh -hex | awk '{print $2}')
//	curl -X POST http://localhost:8081/webhook \
//	    -H "X-FlowCatalyst-Timestamp: $ts" \
//	    -H "X-FlowCatalyst-Signature: $sig" \
//	    -H "Content-Type: application/json" \
//	    -d "$body"
package main

import (
	"errors"
	"io"
	"log"
	"net/http"

	"github.com/flowcatalyst/flowcatalyst-rust/clients/go-sdk/webhook"
)

func main() {
	v, err := webhook.FromEnv() // reads FLOWCATALYST_SIGNING_SECRET
	if err != nil {
		log.Fatalf("webhook validator: %v", err)
	}

	mux := http.NewServeMux()
	mux.HandleFunc("POST /webhook", func(w http.ResponseWriter, r *http.Request) {
		body, err := io.ReadAll(r.Body)
		if err != nil {
			http.Error(w, "read body", http.StatusBadRequest)
			return
		}
		sig := r.Header.Get(webhook.SignatureHeader)
		ts := r.Header.Get(webhook.TimestampHeader)

		if err := v.Validate(sig, ts, body); err != nil {
			// errors.Is is the convention — every webhook error is a
			// declared sentinel.
			switch {
			case errors.Is(err, webhook.ErrMissingSignature),
				errors.Is(err, webhook.ErrMissingTimestamp),
				errors.Is(err, webhook.ErrInvalidTimestamp):
				http.Error(w, err.Error(), http.StatusBadRequest)
			case errors.Is(err, webhook.ErrTimestampExpired),
				errors.Is(err, webhook.ErrTimestampInFuture):
				http.Error(w, err.Error(), http.StatusUnauthorized)
			case errors.Is(err, webhook.ErrInvalidSignature):
				http.Error(w, err.Error(), http.StatusForbidden)
			default:
				http.Error(w, err.Error(), http.StatusInternalServerError)
			}
			return
		}

		// Body is now trusted — parse and process.
		w.WriteHeader(http.StatusOK)
		_, _ = w.Write([]byte("ok\n"))
	})

	log.Println("webhook receiver listening on :8081")
	if err := http.ListenAndServe(":8081", mux); err != nil {
		log.Fatal(err)
	}
}
