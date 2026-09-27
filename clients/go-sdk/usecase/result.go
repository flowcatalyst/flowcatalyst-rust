// Package usecase implements the sealed UseCase + Result pattern used by
// every FlowCatalyst SDK and by the platform itself.
//
// The Result[E] type is a sealed sum: only success[E] and failure[E]
// implement it, both unexported. Failure can be constructed by anyone;
// Success requires a sealed.Token, which only packages under
// clients/go-sdk/ can mint. Therefore the only path to a Success-valued
// Result outside the SDK is through one of the Commit* free functions
// in usecasepgx / usecasesql.
//
// This is what guarantees, at compile time, that every domain event is
// committed atomically with its aggregate state — no code path produces
// a Success without going through a UnitOfWork.
package usecase

import "github.com/flowcatalyst/flowcatalyst-rust/clients/go-sdk/internal/sealed"

// Result is the outcome of a use case execution. It's a sealed sum with
// exactly two implementors: success[E] (constructible only via Success)
// and failure[E] (constructible by anyone via Failure).
type Result[E any] interface {
	isResult()
	unwrap() (E, error)
}

// success holds a successfully-committed event. Unexported on purpose;
// callers outside this package cannot reach it directly.
type success[E any] struct{ event E }

func (success[E]) isResult()            {}
func (s success[E]) unwrap() (E, error) { return s.event, nil }

// failure holds a use case error.
type failure[E any] struct{ err error }

func (failure[E]) isResult()              {}
func (f failure[E]) unwrap() (E, error)   { var zero E; return zero, f.err }

// Success constructs a Result wrapping a value. The sealed.Token is the
// compile-time witness that the caller is internal to the SDK; external
// code cannot import internal/sealed and therefore cannot call this.
//
// SDK-internal callers: pass sealed.New() as the token argument.
func Success[E any](_ sealed.Token, value E) Result[E] {
	return success[E]{event: value}
}

// Failure constructs a Result wrapping an error. Public — anyone can
// return a Failure for validation errors, business rule violations,
// authorization failures, etc.
func Failure[E any](err error) Result[E] {
	return failure[E]{err: err}
}

// Into converts a Result into the stdlib (T, error) shape. Handlers
// call this to turn a use case outcome into an HTTP response.
func Into[E any](r Result[E]) (E, error) {
	return r.unwrap()
}

// IsSuccess reports whether the result is a success.
func IsSuccess[E any](r Result[E]) bool {
	_, ok := r.(success[E])
	return ok
}

// IsFailure reports whether the result is a failure.
func IsFailure[E any](r Result[E]) bool {
	_, ok := r.(failure[E])
	return ok
}
