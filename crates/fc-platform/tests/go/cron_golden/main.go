// Writes tests/data/scheduled_job/cron-go-golden.json: what Go's scheduled-job
// poller (flowcatalyst-go internal/platform/scheduledjob/cron.go) answers for
// a set of crons, walked with robfig/cron v3's SpecSchedule.Next in a set of
// zones from starts around 2026's daylight-saving changes.
//
// Run from this directory, with Go's own tz database so the output does not
// depend on the machine's:
//
//	ZONEINFO=$(go env GOROOT)/lib/time/zoneinfo.zip go run . > ../../data/scheduled_job/cron-go-golden.json
package main

import (
	"encoding/json"
	"os"
	"runtime"
	"time"

	"github.com/robfig/cron/v3"
)

// Go's parser, exactly (cron.go).
var cronParser = cron.NewParser(cron.Second | cron.Minute | cron.Hour | cron.Dom | cron.Month | cron.Dow)

var crons = []string{
	"0 0 * * * *",
	"0 30 * * * *",
	"0 */20 * * * *",
	"0 0 0 * * *",
	"0 30 0 * * *",
	"0 0 1 * * *",
	"0 30 1 * * *",
	"0 0 2 * * *",
	"0 30 2 * * *",
	"0 45 2 * * *",
	"0 0 3 * * *",
	"0 0 23 * * *",
	"0 30 23 * * *",
	"0 0 9 * * 1-5",
	"0 0 12 ? * 0",
	"0 0 0 * * sun",
	"0 0 0 13 * 5",
	"0 30 8 1-7 * mon",
	"0 0 0 */2 * 1",
	"0 0 12 1,15 * *",
	"0 0 0 31 * *",
	"0 0 4 1 jan,jul *",
	"15 */7 */3 * * *",
	"5/20 10 0 * * *",
	"0,,30 0 0 * * *",
	"TZ=Asia/Tokyo 0 0 9 * * *",
	"CRON_TZ=America/New_York 0 30 2 * * *",
}

var zones = []string{
	"UTC",
	"America/New_York",
	"Europe/Amsterdam",
	"Europe/London",
	"Australia/Sydney",
	"Australia/Lord_Howe",
	"Pacific/Chatham",
	"America/Havana",
	"America/Santiago",
	"America/St_Johns",
	"Asia/Kolkata",
}

var starts = []string{
	"2026-03-06T00:00:00Z",
	"2026-03-27T00:00:00Z",
	"2026-04-03T00:00:00Z",
	"2026-09-04T00:00:00Z",
	"2026-09-25T00:00:00Z",
	"2026-10-02T00:00:00Z",
	"2026-10-23T00:00:00Z",
	"2026-10-30T00:00:00Z",
}

// Specs Go's parser reads, or refuses.
var grammar = []string{
	"0 0 * * * *",
	"*/15 * * * * ?",
	"0 30 9 * * MON-FRI",
	"0 0 0 1 JAN,jul *",
	"+5 0 0 * * *",
	"0,,1 * * * * *",
	"0 0 0 * * 1/99999999999",
	"*-5 * * * * *",
	"0 0 0 ? ? ?",
	"TZ=UTC 0 0 9 * * *",
	"TZ= 0 0 9 * * *",
	"",
	"   ",
	"0 * * * *",
	"0 0 * * * * *",
	"@daily",
	"@every 1h",
	"60 * * * * *",
	"0 0 0 0 * *",
	"0 0 5-3 * * *",
	"0 0 0 * * 1/0",
	"0 0 0 * * 7",
	"0/1/2 * * * * *",
	"1-2-3 * * * * *",
	"x * * * * *",
	"0 0 0 * * 1/-1",
	"-1 * * * * *",
	"0 0 0 * * thurs",
	"TZ=Mars/Olympus 0 0 9 * * *",
}

type walk struct {
	Cron  string  `json:"cron"`
	Zone  string  `json:"zone"`
	Start string  `json:"start"`
	Fires []int64 `json:"fires"`
}

type parse struct {
	Cron  string `json:"cron"`
	Error string `json:"error,omitempty"`
}

type golden struct {
	Generator string  `json:"generator"`
	Go        string  `json:"go"`
	Walks     []walk  `json:"walks"`
	Parse     []parse `json:"parse"`
}

const firesPerWalk = 8

func main() {
	out := golden{Generator: "crates/fc-platform/tests/go/cron_golden", Go: runtime.Version()}
	for _, c := range crons {
		sched, err := cronParser.Parse(c)
		if err != nil {
			panic(c + ": " + err.Error())
		}
		for _, z := range zones {
			loc, err := time.LoadLocation(z)
			if err != nil {
				panic(z + ": " + err.Error())
			}
			for _, s := range starts {
				start, _ := time.Parse(time.RFC3339, s)
				w := walk{Cron: c, Zone: z, Start: s, Fires: []int64{}}
				t := start.In(loc)
				for i := 0; i < firesPerWalk; i++ {
					t = sched.Next(t)
					if t.IsZero() {
						break
					}
					w.Fires = append(w.Fires, t.Unix())
				}
				out.Walks = append(out.Walks, w)
			}
		}
	}
	for _, g := range grammar {
		p := parse{Cron: g}
		if _, err := cronParser.Parse(g); err != nil {
			p.Error = err.Error()
		}
		out.Parse = append(out.Parse, p)
	}
	enc := json.NewEncoder(os.Stdout)
	if err := enc.Encode(out); err != nil {
		panic(err)
	}
}
