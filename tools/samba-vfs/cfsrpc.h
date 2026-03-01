/*
 * ClaudeFS RPC Client C FFI Header
 *
 * This header defines the C interface to the ClaudeFS transport layer
 * (claudefs-transport crate). The actual implementation is compiled into
 * libcfsrpc.so from the Rust crate using cbindgen.
 *
 * Error codes
 */

#ifndef CFSRPC_H
#define CFSRPC_H

#include <stdint.h>
#include <stddef.h>
#include <stdbool.h>

#ifdef __cplusplus
extern "C" {
#endif

/* ========================================================================
 * Error codes (match claudefs-transport::error::TransportError variants)
 * ======================================================================== */

#define CFS_ERR_OK              0
#define CFS_ERR_NOT_FOUND       1
#define CFS_ERR_EXISTS          2
#define CFS_ERR_PERMISSION      3
#define CFS_ERR_IO              4
#define CFS_ERR_NO_SPACE        5
#define CFS_ERR_IS_DIR          6
#define CFS_ERR_NOT_DIR         7
#define CFS_ERR_NAME_TOO_LONG   8
#define CFS_ERR_NOT_EMPTY       9
#define CFS_ERR_TOO_MANY_LINKS  10
#define CFS_ERR_TIMEOUT         11
#define CFS_ERR_CONN_REFUSED    12
#define CFS_ERR_EOF             13

/* ========================================================================
 * Opaque handle types
 * ======================================================================== */

/* Opaque RPC connection handle */
typedef struct cfs_rpc_conn cfs_rpc_conn_t;

/* Opaque directory handle */
typedef struct cfs_dir_handle cfs_dir_handle_t;

/* ========================================================================
 * Stat structure (equivalent to struct stat fields used by Samba)
 * ======================================================================== */

typedef struct cfs_stat {
    uint64_t inode;
    uint64_t size;
    uint32_t mode;      /* POSIX mode bits */
    uint32_t nlink;
    uint32_t uid;
    uint32_t gid;
    int64_t  atime_sec;
    int64_t  mtime_sec;
    int64_t  ctime_sec;
} cfs_stat_t;

/* ========================================================================
 * Directory entry
 * ======================================================================== */

typedef struct cfs_dirent {
    uint64_t inode;
    char     name[256];
    bool     is_dir;
    bool     is_symlink;
} cfs_dirent_t;

/* ========================================================================
 * Filesystem statistics (statvfs equivalent)
 * ======================================================================== */

typedef struct cfs_statvfs {
    uint64_t block_size;     /* Block size in bytes */
    uint64_t blocks_total;   /* Total blocks */
    uint64_t blocks_free;    /* Free blocks */
    uint64_t blocks_avail;   /* Blocks available to non-root */
    uint64_t files_total;    /* Total inodes */
    uint64_t files_free;     /* Free inodes */
} cfs_statvfs_t;

/* ========================================================================
 * Connection management
 * ======================================================================== */

/**
 * Establish a connection to a ClaudeFS server.
 *
 * @param addr      Server address (e.g., "cfs-node1:9400")
 * @param timeout_ms Connection timeout in milliseconds
 * @param use_mtls  Whether to use mTLS (requires ~/.cfs/client.crt)
 * @param conn_out  Output: connection handle
 * @return CFS_ERR_OK on success, error code on failure
 */
int cfs_rpc_connect(const char *addr, uint32_t timeout_ms, bool use_mtls,
                     cfs_rpc_conn_t **conn_out);

/**
 * Disconnect from ClaudeFS server and free resources.
 */
void cfs_rpc_disconnect(cfs_rpc_conn_t *conn);

/* ========================================================================
 * Metadata operations
 * ======================================================================== */

int cfs_rpc_stat(cfs_rpc_conn_t *conn, const char *path, cfs_stat_t *out);
int cfs_rpc_fstat(cfs_rpc_conn_t *conn, uint64_t fh, cfs_stat_t *out);
int cfs_rpc_mkdir(cfs_rpc_conn_t *conn, const char *path, uint32_t mode);
int cfs_rpc_rmdir(cfs_rpc_conn_t *conn, const char *path);
int cfs_rpc_unlink(cfs_rpc_conn_t *conn, const char *path);
int cfs_rpc_rename(cfs_rpc_conn_t *conn, const char *src, const char *dst);
int cfs_rpc_statvfs(cfs_rpc_conn_t *conn, const char *path, cfs_statvfs_t *out);

/* ========================================================================
 * File I/O operations
 * ======================================================================== */

/**
 * Open a file.
 *
 * @param conn    Connection handle
 * @param path    Absolute path on ClaudeFS
 * @param flags   Open flags (O_RDONLY, O_WRONLY, O_RDWR, O_CREAT, etc.)
 * @param mode    Creation mode (used when O_CREAT is set)
 * @param fh_out  Output: file handle
 * @return CFS_ERR_OK on success
 */
int cfs_rpc_open(cfs_rpc_conn_t *conn, const char *path, int flags,
                  uint32_t mode, uint64_t *fh_out);

int cfs_rpc_close(cfs_rpc_conn_t *conn, uint64_t fh);

/**
 * Read from an open file.
 *
 * @param conn      Connection handle
 * @param fh        File handle from cfs_rpc_open
 * @param offset    Byte offset (-1 = use current position)
 * @param buf       Output buffer
 * @param len       Bytes to read
 * @param bytes_read Output: actual bytes read
 * @return CFS_ERR_OK on success, CFS_ERR_EOF at end of file
 */
int cfs_rpc_read(cfs_rpc_conn_t *conn, uint64_t fh, int64_t offset,
                  void *buf, size_t len, ssize_t *bytes_read);

/**
 * Write to an open file.
 *
 * @param conn          Connection handle
 * @param fh            File handle from cfs_rpc_open
 * @param offset        Byte offset (-1 = use current position)
 * @param buf           Data to write
 * @param len           Bytes to write
 * @param bytes_written Output: actual bytes written
 * @return CFS_ERR_OK on success
 */
int cfs_rpc_write(cfs_rpc_conn_t *conn, uint64_t fh, int64_t offset,
                   const void *buf, size_t len, ssize_t *bytes_written);

int cfs_rpc_ftruncate(cfs_rpc_conn_t *conn, uint64_t fh, int64_t len);
int cfs_rpc_fsync(cfs_rpc_conn_t *conn, uint64_t fh);

/* ========================================================================
 * Directory operations
 * ======================================================================== */

int cfs_rpc_opendir(cfs_rpc_conn_t *conn, const char *path,
                     cfs_dir_handle_t **dh_out);

/**
 * Read the next directory entry.
 * @return CFS_ERR_OK on success, CFS_ERR_EOF when no more entries
 */
int cfs_rpc_readdir(cfs_rpc_conn_t *conn, cfs_dir_handle_t *dh,
                     cfs_dirent_t *entry_out);

int cfs_rpc_closedir(cfs_rpc_conn_t *conn, cfs_dir_handle_t *dh);

#ifdef __cplusplus
}
#endif

#endif /* CFSRPC_H */
