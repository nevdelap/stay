# stay

## Recovering a deleted tmux socket

If you manually delete tmux's own server socket while a stay session is running,
the session is not lost. Send `SIGUSR1` to the running tmux server and tmux will
recreate the socket in place:

```sh
tmux_pid="$(tmux -L stay display-message -p '#{pid}')"
kill -USR1 "$tmux_pid"
```

Run the first command while the socket is still present. If it has already been
deleted, find the `tmux -L stay` server with `ps` or `pgrep` and send `SIGUSR1`
to that process instead. The server itself remains running when its socket is
deleted, so its sessions and data are preserved.
