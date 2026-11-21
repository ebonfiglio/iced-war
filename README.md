# WAR

Start the server and then open two clients in separate terminals:

```sh
cargo run -p war-server
cargo run -p war-client
```

The server listens on `127.0.0.1:9001`, so both clients must run on the same machine. Connect with distinct usernames, create a room in one client, and join it from the other.

Your seat is at the bottom of the table and your opponent's is at the top. Click the blue deck by your seat or select **Play Card** to play one card per battle. Played cards stay face down until both players have played. The higher rank wins the cards in the pot as points. Ties carry the pot into the next battle; a tie on the final battle splits the pot evenly. After 26 battles, the higher score wins.

If the guest leaves or disconnects, the host returns to the waiting room and can play again with a new guest. If the host leaves or disconnects, the room closes and the guest returns to the lobby.
