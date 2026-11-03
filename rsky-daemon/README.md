# rsky-daemon operations

## Delivery-bucket mirroring

Do not enable `DAEMON_BLOB_FETCH_ENABLED` until the daemon's recovery procedure has completed for every repo in every configured space. The unset delivery bucket is ledger-only mode. If mirroring must be disabled, disable fetch while retaining delivery bucket configuration so orphan cleanup continues; after a rollback past the ledger migration and subsequent roll-forward, run a full recovery before enabling fetch again.

| Variable | Default |
| --- | --- |
| `DAEMON_BLOB_FETCH_ENABLED` | `false` |
| `DAEMON_DELIVERY_BUCKET_ENDPOINT` | unset |
| `DAEMON_DELIVERY_BUCKET_REGION` | unset |
| `DAEMON_DELIVERY_BUCKET_NAME` | unset |
| `DAEMON_DELIVERY_BUCKET_ACCESS_KEY_ID` | unset |
| `DAEMON_DELIVERY_BUCKET_SECRET_ACCESS_KEY` | unset |
| `DAEMON_BLOB_MAX_BYTES` | `20971520` |
| `DAEMON_BLOB_SPACE_QUOTA_BYTES` | `1073741824` |

All delivery bucket variables are required together. When the bucket is unset, fetch cannot be enabled.
