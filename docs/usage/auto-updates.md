# Auto-updates

Available updates are discovered by periodically polling a [Cincinnati] server.
Once available, they are automatically applied via [rpm-ostree] and a machine reboot.

[Cincinnati]: https://github.com/openshift/cincinnati
[rpm-ostree]: https://github.com/projectatomic/rpm-ostree

## Disabling auto-update

To disable auto-updates, a configuration snippet containing the following has to be installed on the system:

```
[updates]
enabled = false
```

Make sure that it has higher priority than previous settings, by using a path like `/etc/zincati/config.d/90-disable-auto-updates.toml`.

When auto-updates are disabled, Zincati does not perform any update action.
However, the service does not terminate and is kept alive idle for external status observers. 
