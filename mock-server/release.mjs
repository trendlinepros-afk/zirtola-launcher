// Make a seeded build the live version (simulates the studio shipping a patch).
//   node mock-server/release.mjs dirt-simulator 1.0.1
const [gameId, version] = process.argv.slice(2);
if (!gameId || !version) {
  console.error("usage: node mock-server/release.mjs <game-id> <version>");
  process.exit(1);
}
const port = process.env.MOCK_PORT || 8787;
const res = await fetch(`http://127.0.0.1:${port}/dev/release`, {
  method: "POST",
  headers: { "Content-Type": "application/json" },
  body: JSON.stringify({ game_id: gameId, version }),
});
console.log(res.status, await res.text());
