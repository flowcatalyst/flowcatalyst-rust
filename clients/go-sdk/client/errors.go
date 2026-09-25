package client

import (
	"encoding/json"
	"fmt"
	"net/http"
)

// APIError is returned when the platform responds with a non-2xx
// status code. Inspect via errors.As(err, &client.APIError{}).
type APIError struct {
	// StatusCode is the HTTP status returned by the platform.
	StatusCode int
	// Body is the raw response body. May contain a structured error
	// envelope (see Code, Message, Details below) or a plain string.
	Body string
}

// Error renders a human-readable summary. Includes the status code and
// the structured Code if one is present in the body.
func (e *APIError) Error() string {
	if code, msg := e.codeAndMessage(); code != "" || msg != "" {
		return fmt.Sprintf("flowcatalyst api: %d %s: %s — %s", e.StatusCode, http.StatusText(e.StatusCode), code, msg)
	}
	if e.Body != "" {
		return fmt.Sprintf("flowcatalyst api: %d %s: %s", e.StatusCode, http.StatusText(e.StatusCode), e.Body)
	}
	return fmt.Sprintf("flowcatalyst api: %d %s", e.StatusCode, http.StatusText(e.StatusCode))
}

// Code returns the platform's error code if the body is a structured
// envelope, or "" if the body is unstructured. The platform sends the code
// in `error` ({"error": "EVENT_TYPE_NOT_FOUND", "message": "..."}); a
// body that carries it in `code` instead is read too.
func (e *APIError) Code() string {
	code, _ := e.codeAndMessage()
	return code
}

// Message returns the platform's error message if present.
func (e *APIError) Message() string {
	_, msg := e.codeAndMessage()
	return msg
}

// Retryable reports whether the caller should retry the request. The
// usual transient statuses (408, 425, 429, 500, 502, 503, 504) qualify.
func (e *APIError) Retryable() bool {
	switch e.StatusCode {
	case http.StatusRequestTimeout,
		http.StatusTooEarly,
		http.StatusTooManyRequests,
		http.StatusInternalServerError,
		http.StatusBadGateway,
		http.StatusServiceUnavailable,
		http.StatusGatewayTimeout:
		return true
	}
	return false
}

// IsNotFound is a convenience for status 404.
func (e *APIError) IsNotFound() bool { return e.StatusCode == http.StatusNotFound }

// IsUnauthorized is a convenience for status 401.
func (e *APIError) IsUnauthorized() bool { return e.StatusCode == http.StatusUnauthorized }

// IsForbidden is a convenience for status 403.
func (e *APIError) IsForbidden() bool { return e.StatusCode == http.StatusForbidden }

// IsConflict is a convenience for status 409.
func (e *APIError) IsConflict() bool { return e.StatusCode == http.StatusConflict }

// codeAndMessage attempts to parse the body as a structured error
// envelope: the code from `error` (the platform's field), else `code`; the
// message from `message`. Falls back to ("","") on any parsing failure.
func (e *APIError) codeAndMessage() (string, string) {
	if e.Body == "" {
		return "", ""
	}
	var env struct {
		Code    string `json:"code"`
		Message string `json:"message"`
		Error   string `json:"error"`
	}
	if err := json.Unmarshal([]byte(e.Body), &env); err != nil {
		return "", ""
	}
	code := env.Error
	if code == "" {
		code = env.Code
	}
	return code, env.Message
}
