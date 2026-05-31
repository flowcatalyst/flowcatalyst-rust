package webhook_test

import (
	"errors"
	"strconv"
	"testing"
	"time"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"

	"github.com/flowcatalyst/flowcatalyst/clients/go-sdk/webhook"
)

func TestValidateAcceptsFreshSignedRequest(t *testing.T) {
	v := webhook.New("test-secret")
	body := []byte(`{"type":"order.created"}`)
	ts := strconv.FormatInt(time.Now().UTC().Unix(), 10)
	sig := v.ComputeSignature(ts, body)

	require.NoError(t, v.Validate(sig, ts, body))
}

// The FlowCatalyst router signs with an ISO8601 millisecond timestamp
// (e.g. 2026-05-24T08:30:00.123Z), not Unix seconds. The validator must
// accept it.
func TestValidateAcceptsISO8601MillisecondTimestamp(t *testing.T) {
	v := webhook.New("test-secret")
	body := []byte(`{"type":"order.created"}`)
	ts := time.Now().UTC().Format("2006-01-02T15:04:05.000Z")
	sig := v.ComputeSignature(ts, body)

	require.NoError(t, v.Validate(sig, ts, body))
}

func TestValidateRejectsExpiredISO8601Timestamp(t *testing.T) {
	v := webhook.New("test-secret")
	body := []byte(`{"x":1}`)
	ts := time.Now().UTC().Add(-time.Hour).Format("2006-01-02T15:04:05.000Z")
	sig := v.ComputeSignature(ts, body)

	assert.True(t, errors.Is(v.Validate(sig, ts, body), webhook.ErrTimestampExpired))
}

func TestValidateRejectsBadSignature(t *testing.T) {
	v := webhook.New("test-secret")
	body := []byte(`{"type":"order.created"}`)
	ts := strconv.FormatInt(time.Now().UTC().Unix(), 10)
	_ = v.ComputeSignature(ts, body) // discard real sig
	err := v.Validate("deadbeef", ts, body)
	assert.True(t, errors.Is(err, webhook.ErrInvalidSignature))
}

func TestValidateRejectsExpiredTimestamp(t *testing.T) {
	v := webhook.New("test-secret")
	body := []byte(`{"x":1}`)
	old := strconv.FormatInt(time.Now().UTC().Add(-time.Hour).Unix(), 10)
	sig := v.ComputeSignature(old, body)
	err := v.Validate(sig, old, body)
	assert.True(t, errors.Is(err, webhook.ErrTimestampExpired))
}

func TestValidateRejectsFutureTimestamp(t *testing.T) {
	v := webhook.New("test-secret")
	body := []byte(`{"x":1}`)
	future := strconv.FormatInt(time.Now().UTC().Add(2*time.Hour).Unix(), 10)
	sig := v.ComputeSignature(future, body)
	err := v.Validate(sig, future, body)
	assert.True(t, errors.Is(err, webhook.ErrTimestampInFuture))
}

func TestValidateRejectsMissingHeaders(t *testing.T) {
	v := webhook.New("test-secret")
	assert.True(t, errors.Is(v.Validate("", "1234", nil), webhook.ErrMissingSignature))
	assert.True(t, errors.Is(v.Validate("sig", "", nil), webhook.ErrMissingTimestamp))
	assert.True(t, errors.Is(v.Validate("sig", "notanumber", nil), webhook.ErrInvalidTimestamp))
}

func TestComputeSignatureIsStable(t *testing.T) {
	v := webhook.New("test-secret")
	a := v.ComputeSignature("100", []byte("hello"))
	b := v.ComputeSignature("100", []byte("hello"))
	assert.Equal(t, a, b)
	assert.NotEqual(t, a, v.ComputeSignature("101", []byte("hello")))
}
