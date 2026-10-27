# Career stats from the game (contract v1)

Games report the player's career stats to the website so they appear on the
player's public profile ("Gold mined", next to "Hours played") and in
Studio → Leaderboard. The first stat set is Dirt Digger's gold. This document is
what the game implements (Unreal C++); the website side is described in the
`retail-store` repo, `docs/LAUNCHER_API.md` → "Career stats".

The short version:

- Started by the launcher → the game has a **launch token** in
  `ZIRTOLA_SESSION_TOKEN`. That token is the credential: send it as
  `Authorization: Bearer …` with every report.
- Every few minutes and at exit, POST **increments** (gold since the last
  report) with a **sequence number** per run. Resending is always safe.
- Can't send? Keep the reports in a file and send them later.
- Not started by the launcher → don't report anything. Play is unaffected.

## 1. How the game receives the session

When the launcher starts the game it sets these environment variables on the
game process (and adds `-zirtola` to the command line):

| Variable | Value | Use for stats |
| --- | --- | --- |
| `ZIRTOLA_SESSION_TOKEN` | launch token: an EdDSA-signed JWT for this account, this game and this run | the `Bearer` credential; its `jti` claim identifies the run |
| `ZIRTOLA_API_BASE` | e.g. `https://www.zirtola.com/api/launcher` | base of the endpoint URL |
| `ZIRTOLA_GAME_ID` | `dirt-simulator` | the `{game}` in the URL |
| `ZIRTOLA_GAME_VERSION` | installed version, e.g. `0.6.0` | `game_version` in the body |
| `ZIRTOLA_PLAY_SESSION_ID` | the launcher's id for this run's playtime (new; launchers before this change don't set it) | `play_session_id` in the body, if set |

Nothing else is needed: no extra key, no sign-in inside the game. The token is
new for every launch, so every run is its own **stats session**.

The token is in the environment rather than on the command line so it doesn't
appear in process lists or crash reports. Keep it that way: never log it,
never put it in a crash report, never show it.

**When to report.** Report only when *this process* was started by the
launcher: `ZIRTOLA_SESSION_TOKEN` is non-empty in the environment **and**
`-zirtola` is on the command line (`FParse::Param(FCommandLine::Get(),
TEXT("zirtola"))`). Do **not** report from a licence token cached on disk for
offline play (the licence check may keep one; it belongs to an earlier run).
Also don't report in the editor, PIE, or non-shipping builds unless a
developer turns it on (see section 9).

If the game was not started by the launcher, the stats system is simply off:
no requests, no queue, no messages, gold works exactly the same in the game.

## 2. What to count (Dirt Digger)

Both values are **grams of fine gold** (the gold itself, not black sand or
concentrate), as decimals with up to 3 places (milligrams). Report **increments**:
what happened since the previous report of this run.

| Field | Meaning in Dirt Digger | Where it happens |
| --- | --- | --- |
| `gold_mined_g` | gold the player won from the ground: caught in the riffles of a sluice/trommel (plus any gold picked up directly, e.g. a nugget) | when gold is added to a plant's recovered gold (`ATrommelActor` stats `GoldRecovered`) |
| `gold_recovered_g` | gold cleaned up out of the plant into the player's savings/gold ledger | when a cleanup adds gold to the player's gold |

Rules:

- Count only gold produced **in this run**. Gold that comes back when a save
  is loaded (e.g. `GoldLoadedG`, riffles restored from a save) is not new and
  must not be reported.
- Never report a negative number. Gold that's sold, spent or lost is not
  "un-mined"; there is nothing to report for it.
- Gold recovered can never exceed gold mined over a career (the website
  enforces that: see section 6), so count the riffle catch as mined even if
  the player hasn't cleaned up yet.
- Never send the player's total. The website keeps the official total and
  ignores client totals.

`active_seconds` is the game's own count of **active play in this run so
far** (a running total, not an increment): time in a loaded site, not paused,
not on menus/loading screens, with player input in the last 5 minutes (the same
idle rule as the launcher). It is used only to check the gold rate.

## 3. Endpoint and payload

```
POST {ZIRTOLA_API_BASE}/v1/games/{game}/stats
Authorization: Bearer {ZIRTOLA_SESSION_TOKEN}
Content-Type: application/json
```

```json
{
  "play_session_id": "Qm9sZC1ydW4tMTIzNDU2",
  "game_version": "0.6.0",
  "reports": [
    { "seq": 1, "gold_mined_g": 1.254, "gold_recovered_g": 0.0, "active_seconds": 300 },
    { "seq": 2, "gold_mined_g": 0.871, "gold_recovered_g": 2.1, "active_seconds": 600 }
  ]
}
```

| Field | Type | Notes |
| --- | --- | --- |
| `play_session_id` | string, optional | `ZIRTOLA_PLAY_SESSION_ID` when set; omit otherwise |
| `game_version` | string, optional | `ZIRTOLA_GAME_VERSION` |
| `reports` | array, 1–50 items | one request = one run (one token) |
| `reports[].seq` | integer 1…1,000,000 | 1, 2, 3 … per run, never reused within the run |
| `reports[].gold_mined_g` | number ≥ 0 | increment since the previous report; ≤ 1000 per report |
| `reports[].gold_recovered_g` | number ≥ 0 | increment since the previous report; ≤ 1000 per report |
| `reports[].active_seconds` | integer ≥ 0 | running total of active play in this run |

A report with both increments zero is fine (it keeps `active_seconds`
current) but you may skip it.

Response `200`:

```json
{
  "results": [
    { "seq": 1, "status": "accepted", "accepted_mined_g": 1.254, "accepted_recovered_g": 0,
      "held_mined_g": 0, "held_recovered_g": 0 },
    { "seq": 2, "status": "duplicate", "accepted_mined_g": 0.871, "accepted_recovered_g": 2.1,
      "held_mined_g": 0, "held_recovered_g": 0 }
  ],
  "session": { "allowed_seconds": 600, "gold_mined_g": 2.125, "gold_recovered_g": 2.1, "held_g": 0, "cap_g": 13.333 },
  "career": { "game_id": "dirt-simulator", "gold_mined_g": 812.4, "gold_mined_ozt": 26.119,
              "gold_recovered_g": 790.1, "gold_recovered_ozt": 25.402 }
}
```

`results` are in the order the reports were sent. `status` is:

| `status` | Meaning | What the game does |
| --- | --- | --- |
| `accepted` | counted in full | remove it from the queue |
| `capped` | counted up to the plausibility cap; the rest is held for review | remove it from the queue (do **not** resend the excess) |
| `duplicate` | this `seq` was already recorded for this run; nothing changed | remove it from the queue |
| `rejected` | bad values (`error`, `message` say why) | remove it and log `message`; resending won't help |

`career` is the official total for the account. The game may show it (for
example "Career: 812.4 g") but must not feed it back into reports.

`GET` on the same URL with the same header returns `{ "career": { … } }`, for
showing the career total at start-up.

## 4. Auth

- `Authorization: Bearer <ZIRTOLA_SESSION_TOKEN>`, over HTTPS. There is no
  separate signing of the body: the token is signed by the website's Ed25519
  launch key, and the website checks its own signature, issuer, `aud` /
  `game`, that the account still owns the game and isn't banned.
- The token's `exp` (5 minutes) only limits *starting* the game. For stats,
  the website accepts it for **30 days after its `iat`**, so a run that ended
  offline can still deliver its queue later. After that the server answers
  `401 session_expired` and the reports are dropped.
- The run is identified by the token's `jti`. The game doesn't need to read
  it: one token = one run = one `seq` counter.
- The launcher's access token is **not** accepted (and the game never has it).

## 5. Batching

- Keep a pending list in memory. Append a report:
  - every **5 minutes** of wall time while a site is loaded (reports with
    nothing new can be skipped),
  - when the player **saves**, returns to the main menu, or the game is about
    to **exit** (`UGameInstance::Shutdown`, `FCoreDelegates::OnPreExit`),
  - after a **cleanup** (a large `gold_recovered_g` step), so it isn't lost
    if the game crashes.
- Each appended report gets the next `seq` and the increments accumulated since
  the previous report; then reset the accumulators.
- **Write the queue file first, then send** (section 6). Send everything
  pending for the run in one request (up to 50 reports; split if more).
- At exit, append the final report, write the queue, try one send with a
  **3-second timeout**, and quit regardless of the result. Never delay or
  block exit on the network.
- At most one request in flight. Never send from the game thread
  synchronously; use the async `FHttpModule` callbacks.

## 6. Offline queue

Reports that haven't been answered must survive a crash, a quit or no
internet.

- File: `%LOCALAPPDATA%/<Project>/Saved/Zirtola/stats-queue.json`
  (`FPaths::ProjectSavedDir() / TEXT("Zirtola/stats-queue.json")`). Write it
  atomically (write `…json.tmp`, then move over the old file).
- Contents: one entry per run, holding the token (it's the credential for
  that run's reports), the run's next `seq`, `play_session_id`,
  `game_version`, and the reports not yet answered:

```json
{
  "version": 1,
  "runs": [
    { "token": "eyJhbGciOiJFZERTQSIs…", "game": "dirt-simulator", "next_seq": 7,
      "play_session_id": "Qm9sZC1ydW4tMTIzNDU2", "game_version": "0.6.0",
      "started_utc": "2026-10-27T18:02:11Z",
      "pending": [ { "seq": 5, "gold_mined_g": 0.42, "gold_recovered_g": 0, "active_seconds": 1500 },
                   { "seq": 6, "gold_mined_g": 0.31, "gold_recovered_g": 0, "active_seconds": 1800 } ] }
  ]
}
```

- Increment `next_seq` and write the file **before** the request goes out,
  so a crash mid-request can't reuse a `seq` (a reused `seq` would be treated
  as a duplicate and its gold lost).
- On start-up (only when started by the launcher, so there is a network
  path and a current run), send each older run's `pending` with **that run's
  token**, oldest run first. Then carry on with the current run.
- Remove a report when it gets any status in `results`. Remove a run when its
  `pending` is empty and it isn't the current run.
- Drop a whole run when the server answers `401` (`session_expired`,
  `unauthorized`) or `403`, or when `started_utc` is more than 30 days ago.
- Cap the file: at most 20 runs and 500 pending reports per run. If a run would
  exceed 500, merge its oldest pending reports into one (sum the increments,
  keep the larger `active_seconds`, keep the lowest `seq`) rather than losing
  gold.
- The file holds launch tokens: keep it in the user's own profile folder (it
  is), and never upload it with crash reports.

## 7. Error handling

| Response | Meaning | Action |
| --- | --- | --- |
| `200` | handled; see `results` | remove answered reports |
| `400 bad_request` | the request itself was malformed (no `reports`, more than 50) | fix the batch (split it); a bug if it happens |
| `401 unauthorized` | token missing, forged, for another game, or from the future (PC clock) | drop this run's queue; log once |
| `401 session_expired` | the run started more than 30 days ago | drop this run's queue |
| `403 not_entitled` / `forbidden` | the account no longer owns the game, or is banned | drop this run's queue |
| `404 not_found` | unknown game id | drop; log once |
| `429 slow_down` | more than 240 requests an hour for this account | keep the queue, wait 15 minutes |
| `503 launcher_not_configured` | website misconfigured | keep the queue, retry later |
| `5xx`, timeout, no network | transient | keep the queue; retry with backoff: 30 s, 1, 2, 5, 10, then every 15 min; and at the next start |

Never show these errors to the player as blocking dialogs. At most, a small
note in the pause menu ("Career stats will sync when you're online"). Gold in
the game never depends on the server's answer.

## 8. What the website does with reports (so the numbers make sense)

- **Server-authoritative totals.** The career total is the sum of accepted
  increments. Client totals are never read.
- **Idempotent.** `(run, seq)` is unique; resends answer `duplicate`.
- **Plausibility.** For each run, accepted gold mined stays under
  `50 g/h × counted hours + 5 g` (both are website settings:
  `CAREER_GOLD_MAX_GRAMS_PER_HOUR`, `CAREER_GOLD_BURST_GRAMS`). Counted time is
  the game's `active_seconds`, but never more than the time since the website
  signed the launch token, and, when `play_session_id` matches the launcher's
  playtime report for the same run, never more than the launcher measured
  (+10 minutes for its reporting lag). Gold recovered can't exceed career gold
  mined (+5 g). Over a limit, the report is **accepted up to the cap and the
  excess is held** and flagged for Studio to release or dismiss. So: report
  real numbers and real active time; a faster-than-possible rate is never
  counted publicly.
- **Privacy.** Players can hide "Gold mined" on their profile (Settings);
  Studio always sees it. The game doesn't need to do anything about it.

## 9. Unreal C++ example

Add `"HTTP"`, `"Json"` and `"JsonUtilities"` to the module's
`PublicDependencyModuleNames`. A minimal sender (queue file handling trimmed
to the essentials):

```cpp
// ZirtolaStats.h
#pragma once
#include "CoreMinimal.h"

struct FZirtolaStatReport
{
    int32 Seq = 0;
    double GoldMinedG = 0.0;      // increment since the previous report
    double GoldRecoveredG = 0.0;  // increment since the previous report
    int32 ActiveSeconds = 0;      // running total for this run
};

class FZirtolaStats
{
public:
    /** True when this process was started by the Zirtola Launcher. */
    static bool IsLaunchedByLauncher();

    /** Send the pending reports of one run; OnDone gets the seqs to remove from the queue, or bDropRun. */
    static void Send(const FString& Token, const FString& PlaySessionId, const FString& GameVersion,
                     const TArray<FZirtolaStatReport>& Pending,
                     TFunction<void(const TArray<int32>& Answered, bool bDropRun)> OnDone);
};
```

```cpp
// ZirtolaStats.cpp
#include "ZirtolaStats.h"
#include "HttpModule.h"
#include "Interfaces/IHttpRequest.h"
#include "Interfaces/IHttpResponse.h"
#include "Dom/JsonObject.h"
#include "Serialization/JsonReader.h"
#include "Serialization/JsonSerializer.h"
#include "Misc/CommandLine.h"

static const TCHAR* GZirtolaGameId = TEXT("dirt-simulator");

bool FZirtolaStats::IsLaunchedByLauncher()
{
#if !UE_BUILD_SHIPPING
    // Developers opt in with -zirtolastats, so editor/dev runs never report by accident.
    if (!FParse::Param(FCommandLine::Get(), TEXT("zirtolastats"))) return false;
#endif
    return FParse::Param(FCommandLine::Get(), TEXT("zirtola"))
        && !FPlatformMisc::GetEnvironmentVariable(TEXT("ZIRTOLA_SESSION_TOKEN")).IsEmpty();
}

void FZirtolaStats::Send(const FString& Token, const FString& PlaySessionId, const FString& GameVersion,
                         const TArray<FZirtolaStatReport>& Pending,
                         TFunction<void(const TArray<int32>&, bool)> OnDone)
{
    FString Api = FPlatformMisc::GetEnvironmentVariable(TEXT("ZIRTOLA_API_BASE"));
    if (Api.IsEmpty()) Api = TEXT("https://www.zirtola.com/api/launcher");

    // Body: { play_session_id?, game_version, reports: [ … ] } (at most 50 reports)
    TSharedRef<FJsonObject> Body = MakeShared<FJsonObject>();
    if (!PlaySessionId.IsEmpty()) Body->SetStringField(TEXT("play_session_id"), PlaySessionId);
    Body->SetStringField(TEXT("game_version"), GameVersion);
    TArray<TSharedPtr<FJsonValue>> Reports;
    for (int32 i = 0; i < Pending.Num() && i < 50; ++i)
    {
        const FZirtolaStatReport& R = Pending[i];
        TSharedRef<FJsonObject> J = MakeShared<FJsonObject>();
        J->SetNumberField(TEXT("seq"), R.Seq);
        // Milligram precision; never negative.
        J->SetNumberField(TEXT("gold_mined_g"), FMath::RoundToDouble(FMath::Max(0.0, R.GoldMinedG) * 1000.0) / 1000.0);
        J->SetNumberField(TEXT("gold_recovered_g"), FMath::RoundToDouble(FMath::Max(0.0, R.GoldRecoveredG) * 1000.0) / 1000.0);
        J->SetNumberField(TEXT("active_seconds"), FMath::Max(0, R.ActiveSeconds));
        Reports.Add(MakeShared<FJsonValueObject>(J));
    }
    Body->SetArrayField(TEXT("reports"), Reports);
    FString Json;
    FJsonSerializer::Serialize(Body, TJsonWriterFactory<>::Create(&Json));

    TSharedRef<IHttpRequest, ESPMode::ThreadSafe> Req = FHttpModule::Get().CreateRequest();
    Req->SetURL(Api / TEXT("v1/games") / GZirtolaGameId / TEXT("stats"));
    Req->SetVerb(TEXT("POST"));
    Req->SetHeader(TEXT("Authorization"), TEXT("Bearer ") + Token);  // never log this header
    Req->SetHeader(TEXT("Content-Type"), TEXT("application/json"));
    Req->SetContentAsString(Json);
    Req->SetTimeout(15.0f);  // 3 s at exit
    Req->OnProcessRequestComplete().BindLambda(
        [OnDone](FHttpRequestPtr, FHttpResponsePtr Resp, bool bOk)
        {
            TArray<int32> Answered;
            if (!bOk || !Resp.IsValid()) { OnDone(Answered, false); return; }   // offline: keep, retry later
            const int32 Code = Resp->GetResponseCode();
            if (Code == 401 || Code == 403 || Code == 404) { OnDone(Answered, true); return; }  // drop this run
            if (Code != 200) { OnDone(Answered, false); return; }                // 429/5xx: keep, back off

            TSharedPtr<FJsonObject> Root;
            if (FJsonSerializer::Deserialize(TJsonReaderFactory<>::Create(Resp->GetContentAsString()), Root) && Root.IsValid())
            {
                const TArray<TSharedPtr<FJsonValue>>* Results = nullptr;
                if (Root->TryGetArrayField(TEXT("results"), Results))
                {
                    for (const TSharedPtr<FJsonValue>& V : *Results)
                    {
                        const TSharedPtr<FJsonObject> O = V->AsObject();
                        int32 Seq = 0;
                        if (O.IsValid() && O->TryGetNumberField(TEXT("seq"), Seq)) Answered.Add(Seq);  // any status = answered
                    }
                }
                // Root->GetObjectField(TEXT("career")) has the official totals for display.
            }
            OnDone(Answered, false);
        });
    Req->ProcessRequest();
}
```

Typical use, from a `UGameInstanceSubsystem`:

```cpp
// Initialize(): only when started by the launcher
bEnabled = FZirtolaStats::IsLaunchedByLauncher();
if (bEnabled)
{
    Token = FPlatformMisc::GetEnvironmentVariable(TEXT("ZIRTOLA_SESSION_TOKEN"));
    PlaySessionId = FPlatformMisc::GetEnvironmentVariable(TEXT("ZIRTOLA_PLAY_SESSION_ID"));
    LoadQueue();            // older runs first: send them with their own tokens
    AddCurrentRunToQueue(); // { token, next_seq = 1, … }
}

// Gameplay hooks add to the accumulators (only when bEnabled):
//   OnGoldCaught(double Grams)     { PendingMinedG += Grams; }      // riffle catch, nuggets
//   OnGoldCleanedUp(double Grams)  { PendingRecoveredG += Grams; }  // cleanup into savings

// Every 5 min, on save, after a cleanup and at exit:
FZirtolaStatReport R;
R.Seq = NextSeq++;
R.GoldMinedG = PendingMinedG;  R.GoldRecoveredG = PendingRecoveredG;
R.ActiveSeconds = ActivePlaySeconds;
PendingMinedG = PendingRecoveredG = 0.0;
CurrentRun.Pending.Add(R);
SaveQueue();                     // before sending
if (!bInFlight) { bInFlight = true; FZirtolaStats::Send(Token, PlaySessionId, Version, CurrentRun.Pending,
    [this](const TArray<int32>& Answered, bool bDropRun) { bInFlight = false; RemoveAnswered(Answered, bDropRun); SaveQueue(); }); }
```

## 10. Testing

- **Mock server** (this repo): `npm run mock:seed && npm run mock`, then run
  the game with `ZIRTOLA_API_BASE=http://127.0.0.1:8787` and a token from the
  launcher in dev mode. The mock implements this endpoint with the same rules
  (in memory) and logs each report: `[mock] stats … #3: +0.42 g mined …`.
  `MOCK_GOLD_MAX_GRAMS_PER_HOUR` changes the cap.
- **Local website**: `retail-store` repo, `npm run test:stats` covers auth,
  idempotency, the cap, privacy and the Studio leaderboard.
- Things to try in the game: pull the network cable mid-run and quit (the queue
  file keeps the reports; the next launch sends them); kill the process
  mid-request (the `seq` isn't reused); start the exe directly without the
  launcher (no requests, no file).
