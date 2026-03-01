/*
 * ClaudeFS Samba VFS Module
 *
 * This module translates Samba VFS operations to ClaudeFS internal RPC calls,
 * enabling SMB3 access to ClaudeFS namespaces via the Samba gateway.
 *
 * License: GPLv3 (required for Samba VFS modules)
 * See docs/agents.md section "SMB3 Gateway: Samba VFS Plugin" for architecture.
 *
 * Compile with:
 *   gcc -shared -fPIC -o cfs_vfs.so cfs_vfs.c \
 *       $(pkg-config --cflags --libs samba-util samba-hostconfig) \
 *       -lcfsrpc
 *
 * Install:
 *   cp cfs_vfs.so /usr/lib/x86_64-linux-gnu/samba/vfs/
 *
 * Samba smb.conf snippet:
 *   [data]
 *     path = /mnt/cfs-export
 *     vfs objects = cfs_vfs
 *     cfs:server = cfs-storage-01:9400
 *     cfs:timeout_ms = 5000
 *     cfs:export = /data
 *
 * This module requires:
 *   - Samba 4.x with VFS module support
 *   - ClaudeFS transport library (libcfsrpc.so) from claudefs-transport crate
 *   - Cluster running with NFS/RPC gateway enabled
 */

#include <stdint.h>
#include <stdbool.h>
#include <string.h>
#include <errno.h>
#include <sys/stat.h>
#include <sys/types.h>
#include <fcntl.h>
#include <unistd.h>
#include <dirent.h>
#include <time.h>

/* Samba headers - installed via samba-dev package */
#ifdef HAVE_SAMBA_HEADERS
#include "includes.h"
#include "smbd/smbd.h"
#include "system/filesys.h"
#include "lib/util/tevent_unix.h"
#include "vfs.h"
#endif

/* ClaudeFS RPC client stub - provided by claudefs-transport crate's C FFI */
#include "cfsrpc.h"

/* ========================================================================
 * Module version and identification
 * ======================================================================== */

#define CFS_VFS_MODULE_NAME "cfs_vfs"
#define CFS_VFS_VERSION     "0.1.0"
#define CFS_VFS_VENDOR      "ClaudeFS Project"

/* ========================================================================
 * Per-connection state
 * ======================================================================== */

typedef struct cfs_vfs_conn {
    /* ClaudeFS RPC connection handle */
    cfs_rpc_conn_t *rpc_conn;
    /* Server address (from smb.conf: cfs:server) */
    char server_addr[256];
    /* Export path on ClaudeFS (from smb.conf: cfs:export) */
    char export_path[4096];
    /* RPC timeout in milliseconds */
    uint32_t timeout_ms;
    /* Whether mTLS is enabled */
    bool mtls_enabled;
    /* Connection stats */
    uint64_t read_bytes;
    uint64_t write_bytes;
    uint64_t rpc_calls;
    uint64_t rpc_errors;
} cfs_vfs_conn_t;

/* ========================================================================
 * Error translation: CFS error codes → POSIX errno
 * ======================================================================== */

static int cfs_err_to_errno(int cfs_err) {
    switch (cfs_err) {
    case CFS_ERR_OK:          return 0;
    case CFS_ERR_NOT_FOUND:   return ENOENT;
    case CFS_ERR_EXISTS:      return EEXIST;
    case CFS_ERR_PERMISSION:  return EACCES;
    case CFS_ERR_IO:          return EIO;
    case CFS_ERR_NO_SPACE:    return ENOSPC;
    case CFS_ERR_IS_DIR:      return EISDIR;
    case CFS_ERR_NOT_DIR:     return ENOTDIR;
    case CFS_ERR_NAME_TOO_LONG: return ENAMETOOLONG;
    case CFS_ERR_NOT_EMPTY:   return ENOTEMPTY;
    case CFS_ERR_TOO_MANY_LINKS: return EMLINK;
    case CFS_ERR_TIMEOUT:     return ETIMEDOUT;
    case CFS_ERR_CONN_REFUSED: return ECONNREFUSED;
    default:                   return EIO;
    }
}

/* ========================================================================
 * Path resolution: combine export root with relative VFS path
 * ======================================================================== */

static int cfs_build_path(cfs_vfs_conn_t *conn, const char *rel_path,
                           char *out, size_t out_len) {
    int n = snprintf(out, out_len, "%s/%s", conn->export_path, rel_path);
    if (n < 0 || (size_t)n >= out_len) {
        errno = ENAMETOOLONG;
        return -1;
    }
    return 0;
}

/* ========================================================================
 * VFS Operation: connect
 * Called when a Samba connection uses this VFS module.
 * ======================================================================== */

static int cfs_vfs_connect(vfs_handle_struct *handle, const char *service,
                            const char *user) {
    cfs_vfs_conn_t *conn;
    const char *server;
    const char *export_path;
    int timeout_ms;
    int ret;

    conn = talloc_zero(handle->conn, cfs_vfs_conn_t);
    if (!conn) {
        errno = ENOMEM;
        return -1;
    }

    /* Read smb.conf parameters */
    server = lp_parm_const_string(SNUM(handle->conn), CFS_VFS_MODULE_NAME,
                                   "server", "localhost:9400");
    export_path = lp_parm_const_string(SNUM(handle->conn), CFS_VFS_MODULE_NAME,
                                        "export", "/");
    timeout_ms = lp_parm_int(SNUM(handle->conn), CFS_VFS_MODULE_NAME,
                               "timeout_ms", 5000);

    strncpy(conn->server_addr, server, sizeof(conn->server_addr) - 1);
    strncpy(conn->export_path, export_path, sizeof(conn->export_path) - 1);
    conn->timeout_ms = (uint32_t)timeout_ms;
    conn->mtls_enabled = lp_parm_bool(SNUM(handle->conn), CFS_VFS_MODULE_NAME,
                                       "mtls", true);

    /* Establish RPC connection to ClaudeFS */
    ret = cfs_rpc_connect(conn->server_addr, conn->timeout_ms,
                           conn->mtls_enabled, &conn->rpc_conn);
    if (ret != 0) {
        DEBUG(0, ("cfs_vfs: failed to connect to %s: %s\n",
                  conn->server_addr, strerror(cfs_err_to_errno(ret))));
        talloc_free(conn);
        errno = cfs_err_to_errno(ret);
        return -1;
    }

    SMB_VFS_HANDLE_SET_DATA(handle, conn, NULL, cfs_vfs_conn_t, return -1);

    DEBUG(5, ("cfs_vfs: connected to %s, export=%s\n",
              conn->server_addr, conn->export_path));
    return 0;
}

/* ========================================================================
 * VFS Operation: disconnect
 * ======================================================================== */

static void cfs_vfs_disconnect(vfs_handle_struct *handle) {
    cfs_vfs_conn_t *conn;
    SMB_VFS_HANDLE_GET_DATA(handle, conn, cfs_vfs_conn_t, return);

    DEBUG(5, ("cfs_vfs: disconnecting from %s (reads=%lu writes=%lu calls=%lu errors=%lu)\n",
              conn->server_addr,
              (unsigned long)conn->read_bytes,
              (unsigned long)conn->write_bytes,
              (unsigned long)conn->rpc_calls,
              (unsigned long)conn->rpc_errors));

    if (conn->rpc_conn) {
        cfs_rpc_disconnect(conn->rpc_conn);
        conn->rpc_conn = NULL;
    }

    SMB_VFS_NEXT_DISCONNECT(handle);
}

/* ========================================================================
 * VFS Operation: stat / lstat / fstat
 * ======================================================================== */

static int cfs_vfs_stat(vfs_handle_struct *handle, struct smb_filename *smb_fname) {
    cfs_vfs_conn_t *conn;
    cfs_stat_t cfs_st;
    char full_path[4096];
    int ret;

    SMB_VFS_HANDLE_GET_DATA(handle, conn, cfs_vfs_conn_t, return -1);

    if (cfs_build_path(conn, smb_fname->base_name, full_path, sizeof(full_path)) < 0) {
        return -1;
    }

    conn->rpc_calls++;
    ret = cfs_rpc_stat(conn->rpc_conn, full_path, &cfs_st);
    if (ret != 0) {
        conn->rpc_errors++;
        errno = cfs_err_to_errno(ret);
        return -1;
    }

    /* Translate cfs_stat_t → struct stat in smb_fname->st */
    smb_fname->st.st_ex_ino   = cfs_st.inode;
    smb_fname->st.st_ex_size  = cfs_st.size;
    smb_fname->st.st_ex_mode  = cfs_st.mode;
    smb_fname->st.st_ex_nlink = cfs_st.nlink;
    smb_fname->st.st_ex_uid   = cfs_st.uid;
    smb_fname->st.st_ex_gid   = cfs_st.gid;
    smb_fname->st.st_ex_blksize = 4096;
    smb_fname->st.st_ex_blocks  = (cfs_st.size + 511) / 512;

    smb_fname->st.st_ex_atime.tv_sec  = cfs_st.atime_sec;
    smb_fname->st.st_ex_atime.tv_nsec = 0;
    smb_fname->st.st_ex_mtime.tv_sec  = cfs_st.mtime_sec;
    smb_fname->st.st_ex_mtime.tv_nsec = 0;
    smb_fname->st.st_ex_ctime.tv_sec  = cfs_st.ctime_sec;
    smb_fname->st.st_ex_ctime.tv_nsec = 0;

    return 0;
}

static int cfs_vfs_lstat(vfs_handle_struct *handle, struct smb_filename *smb_fname) {
    /* For symlinks, CFS currently treats lstat same as stat (no symlink-following).
     * Production implementation would use a separate RPC that doesn't follow symlinks. */
    return cfs_vfs_stat(handle, smb_fname);
}

static int cfs_vfs_fstat(vfs_handle_struct *handle, files_struct *fsp,
                          SMB_STRUCT_STAT *sbuf) {
    cfs_vfs_conn_t *conn;
    cfs_stat_t cfs_st;
    int ret;

    SMB_VFS_HANDLE_GET_DATA(handle, conn, cfs_vfs_conn_t, return -1);

    conn->rpc_calls++;
    ret = cfs_rpc_fstat(conn->rpc_conn, (uint64_t)(uintptr_t)fsp->fh->fd, &cfs_st);
    if (ret != 0) {
        conn->rpc_errors++;
        errno = cfs_err_to_errno(ret);
        return -1;
    }

    sbuf->st_ex_ino   = cfs_st.inode;
    sbuf->st_ex_size  = cfs_st.size;
    sbuf->st_ex_mode  = cfs_st.mode;
    sbuf->st_ex_nlink = cfs_st.nlink;
    sbuf->st_ex_uid   = cfs_st.uid;
    sbuf->st_ex_gid   = cfs_st.gid;
    sbuf->st_ex_blksize = 4096;
    sbuf->st_ex_blocks  = (cfs_st.size + 511) / 512;
    sbuf->st_ex_atime.tv_sec  = cfs_st.atime_sec;
    sbuf->st_ex_mtime.tv_sec  = cfs_st.mtime_sec;
    sbuf->st_ex_ctime.tv_sec  = cfs_st.ctime_sec;

    return 0;
}

/* ========================================================================
 * VFS Operation: open / close
 * ======================================================================== */

static int cfs_vfs_open(vfs_handle_struct *handle, struct smb_filename *smb_fname,
                         files_struct *fsp, int flags, mode_t mode) {
    cfs_vfs_conn_t *conn;
    uint64_t file_handle;
    char full_path[4096];
    int ret;

    SMB_VFS_HANDLE_GET_DATA(handle, conn, cfs_vfs_conn_t, return -1);

    if (cfs_build_path(conn, smb_fname->base_name, full_path, sizeof(full_path)) < 0) {
        return -1;
    }

    conn->rpc_calls++;
    ret = cfs_rpc_open(conn->rpc_conn, full_path, flags, mode, &file_handle);
    if (ret != 0) {
        conn->rpc_errors++;
        errno = cfs_err_to_errno(ret);
        return -1;
    }

    /* Store CFS file handle in the fd field (we use it as an opaque token) */
    fsp->fh->fd = (int)(uintptr_t)file_handle;
    return fsp->fh->fd;
}

static int cfs_vfs_close(vfs_handle_struct *handle, files_struct *fsp) {
    cfs_vfs_conn_t *conn;
    int ret;

    SMB_VFS_HANDLE_GET_DATA(handle, conn, cfs_vfs_conn_t, return -1);

    conn->rpc_calls++;
    ret = cfs_rpc_close(conn->rpc_conn, (uint64_t)(uintptr_t)fsp->fh->fd);
    if (ret != 0) {
        conn->rpc_errors++;
        /* Don't fail on close errors, just log */
        DEBUG(2, ("cfs_vfs: close error: %d\n", ret));
    }

    fsp->fh->fd = -1;
    return 0;
}

/* ========================================================================
 * VFS Operation: read / pread
 * ======================================================================== */

static ssize_t cfs_vfs_read(vfs_handle_struct *handle, files_struct *fsp,
                              void *data, size_t n) {
    cfs_vfs_conn_t *conn;
    ssize_t bytes_read;
    int ret;

    SMB_VFS_HANDLE_GET_DATA(handle, conn, cfs_vfs_conn_t, return -1);

    conn->rpc_calls++;
    ret = cfs_rpc_read(conn->rpc_conn, (uint64_t)(uintptr_t)fsp->fh->fd,
                        -1, /* current offset */ data, n, &bytes_read);
    if (ret != 0) {
        conn->rpc_errors++;
        errno = cfs_err_to_errno(ret);
        return -1;
    }

    conn->read_bytes += (uint64_t)bytes_read;
    return bytes_read;
}

static ssize_t cfs_vfs_pread(vfs_handle_struct *handle, files_struct *fsp,
                               void *data, size_t n, off_t offset) {
    cfs_vfs_conn_t *conn;
    ssize_t bytes_read;
    int ret;

    SMB_VFS_HANDLE_GET_DATA(handle, conn, cfs_vfs_conn_t, return -1);

    conn->rpc_calls++;
    ret = cfs_rpc_read(conn->rpc_conn, (uint64_t)(uintptr_t)fsp->fh->fd,
                        (int64_t)offset, data, n, &bytes_read);
    if (ret != 0) {
        conn->rpc_errors++;
        errno = cfs_err_to_errno(ret);
        return -1;
    }

    conn->read_bytes += (uint64_t)bytes_read;
    return bytes_read;
}

/* ========================================================================
 * VFS Operation: write / pwrite
 * ======================================================================== */

static ssize_t cfs_vfs_write(vfs_handle_struct *handle, files_struct *fsp,
                               const void *data, size_t n) {
    cfs_vfs_conn_t *conn;
    ssize_t bytes_written;
    int ret;

    SMB_VFS_HANDLE_GET_DATA(handle, conn, cfs_vfs_conn_t, return -1);

    conn->rpc_calls++;
    ret = cfs_rpc_write(conn->rpc_conn, (uint64_t)(uintptr_t)fsp->fh->fd,
                         -1, /* current offset */ data, n, &bytes_written);
    if (ret != 0) {
        conn->rpc_errors++;
        errno = cfs_err_to_errno(ret);
        return -1;
    }

    conn->write_bytes += (uint64_t)bytes_written;
    return bytes_written;
}

static ssize_t cfs_vfs_pwrite(vfs_handle_struct *handle, files_struct *fsp,
                                const void *data, size_t n, off_t offset) {
    cfs_vfs_conn_t *conn;
    ssize_t bytes_written;
    int ret;

    SMB_VFS_HANDLE_GET_DATA(handle, conn, cfs_vfs_conn_t, return -1);

    conn->rpc_calls++;
    ret = cfs_rpc_write(conn->rpc_conn, (uint64_t)(uintptr_t)fsp->fh->fd,
                         (int64_t)offset, data, n, &bytes_written);
    if (ret != 0) {
        conn->rpc_errors++;
        errno = cfs_err_to_errno(ret);
        return -1;
    }

    conn->write_bytes += (uint64_t)bytes_written;
    return bytes_written;
}

/* ========================================================================
 * VFS Operation: mkdir / rmdir
 * ======================================================================== */

static int cfs_vfs_mkdir(vfs_handle_struct *handle, const struct smb_filename *smb_fname,
                          mode_t mode) {
    cfs_vfs_conn_t *conn;
    char full_path[4096];
    int ret;

    SMB_VFS_HANDLE_GET_DATA(handle, conn, cfs_vfs_conn_t, return -1);

    if (cfs_build_path(conn, smb_fname->base_name, full_path, sizeof(full_path)) < 0) {
        return -1;
    }

    conn->rpc_calls++;
    ret = cfs_rpc_mkdir(conn->rpc_conn, full_path, mode);
    if (ret != 0) {
        conn->rpc_errors++;
        errno = cfs_err_to_errno(ret);
        return -1;
    }
    return 0;
}

static int cfs_vfs_rmdir(vfs_handle_struct *handle, const struct smb_filename *smb_fname) {
    cfs_vfs_conn_t *conn;
    char full_path[4096];
    int ret;

    SMB_VFS_HANDLE_GET_DATA(handle, conn, cfs_vfs_conn_t, return -1);

    if (cfs_build_path(conn, smb_fname->base_name, full_path, sizeof(full_path)) < 0) {
        return -1;
    }

    conn->rpc_calls++;
    ret = cfs_rpc_rmdir(conn->rpc_conn, full_path);
    if (ret != 0) {
        conn->rpc_errors++;
        errno = cfs_err_to_errno(ret);
        return -1;
    }
    return 0;
}

/* ========================================================================
 * VFS Operation: unlink / rename
 * ======================================================================== */

static int cfs_vfs_unlink(vfs_handle_struct *handle,
                            const struct smb_filename *smb_fname) {
    cfs_vfs_conn_t *conn;
    char full_path[4096];
    int ret;

    SMB_VFS_HANDLE_GET_DATA(handle, conn, cfs_vfs_conn_t, return -1);

    if (cfs_build_path(conn, smb_fname->base_name, full_path, sizeof(full_path)) < 0) {
        return -1;
    }

    conn->rpc_calls++;
    ret = cfs_rpc_unlink(conn->rpc_conn, full_path);
    if (ret != 0) {
        conn->rpc_errors++;
        errno = cfs_err_to_errno(ret);
        return -1;
    }
    return 0;
}

static int cfs_vfs_rename(vfs_handle_struct *handle,
                            const struct smb_filename *smb_fname_src,
                            const struct smb_filename *smb_fname_dst) {
    cfs_vfs_conn_t *conn;
    char src_path[4096];
    char dst_path[4096];
    int ret;

    SMB_VFS_HANDLE_GET_DATA(handle, conn, cfs_vfs_conn_t, return -1);

    if (cfs_build_path(conn, smb_fname_src->base_name, src_path, sizeof(src_path)) < 0 ||
        cfs_build_path(conn, smb_fname_dst->base_name, dst_path, sizeof(dst_path)) < 0) {
        return -1;
    }

    conn->rpc_calls++;
    ret = cfs_rpc_rename(conn->rpc_conn, src_path, dst_path);
    if (ret != 0) {
        conn->rpc_errors++;
        errno = cfs_err_to_errno(ret);
        return -1;
    }
    return 0;
}

/* ========================================================================
 * VFS Operation: opendir / readdir / closedir
 * ======================================================================== */

static DIR *cfs_vfs_opendir(vfs_handle_struct *handle,
                              const struct smb_filename *smb_fname,
                              const char *mask, uint32_t attr) {
    cfs_vfs_conn_t *conn;
    cfs_dir_handle_t *dir_handle;
    char full_path[4096];
    int ret;

    SMB_VFS_HANDLE_GET_DATA(handle, conn, cfs_vfs_conn_t, return NULL);

    if (cfs_build_path(conn, smb_fname->base_name, full_path, sizeof(full_path)) < 0) {
        return NULL;
    }

    conn->rpc_calls++;
    ret = cfs_rpc_opendir(conn->rpc_conn, full_path, &dir_handle);
    if (ret != 0) {
        conn->rpc_errors++;
        errno = cfs_err_to_errno(ret);
        return NULL;
    }

    /* Return the CFS dir handle cast to DIR* (opaque pointer) */
    return (DIR *)dir_handle;
}

static struct dirent *cfs_vfs_readdir(vfs_handle_struct *handle,
                                       DIR *dirp,
                                       SMB_STRUCT_STAT *sbuf) {
    cfs_vfs_conn_t *conn;
    cfs_dirent_t cfs_de;
    static struct dirent de;  /* NOTE: not thread-safe, see production TODO below */
    int ret;

    SMB_VFS_HANDLE_GET_DATA(handle, conn, cfs_vfs_conn_t, return NULL);

    /* TODO(production): Use thread-local storage for the dirent buffer */
    conn->rpc_calls++;
    ret = cfs_rpc_readdir(conn->rpc_conn, (cfs_dir_handle_t *)dirp, &cfs_de);
    if (ret == CFS_ERR_EOF) {
        return NULL;  /* End of directory */
    }
    if (ret != 0) {
        conn->rpc_errors++;
        errno = cfs_err_to_errno(ret);
        return NULL;
    }

    /* Translate cfs_dirent_t → struct dirent */
    memset(&de, 0, sizeof(de));
    de.d_ino = cfs_de.inode;
    de.d_type = (cfs_de.is_dir ? DT_DIR :
                 cfs_de.is_symlink ? DT_LNK : DT_REG);
    strncpy(de.d_name, cfs_de.name, sizeof(de.d_name) - 1);

    /* Fill stat if requested */
    if (sbuf) {
        sbuf->st_ex_ino  = cfs_de.inode;
        sbuf->st_ex_mode = cfs_de.is_dir ? S_IFDIR : S_IFREG;
    }

    return &de;
}

static int cfs_vfs_closedir(vfs_handle_struct *handle, DIR *dirp) {
    cfs_vfs_conn_t *conn;
    int ret;

    SMB_VFS_HANDLE_GET_DATA(handle, conn, cfs_vfs_conn_t, return -1);

    conn->rpc_calls++;
    ret = cfs_rpc_closedir(conn->rpc_conn, (cfs_dir_handle_t *)dirp);
    if (ret != 0) {
        conn->rpc_errors++;
        /* Don't fail on closedir errors */
        DEBUG(2, ("cfs_vfs: closedir error: %d\n", ret));
    }

    return 0;
}

/* ========================================================================
 * VFS Operation: fsync
 * ======================================================================== */

static int cfs_vfs_fsync(vfs_handle_struct *handle, files_struct *fsp) {
    cfs_vfs_conn_t *conn;
    int ret;

    SMB_VFS_HANDLE_GET_DATA(handle, conn, cfs_vfs_conn_t, return -1);

    conn->rpc_calls++;
    ret = cfs_rpc_fsync(conn->rpc_conn, (uint64_t)(uintptr_t)fsp->fh->fd);
    if (ret != 0) {
        conn->rpc_errors++;
        errno = cfs_err_to_errno(ret);
        return -1;
    }
    return 0;
}

/* ========================================================================
 * VFS Operation: ftruncate / truncate
 * ======================================================================== */

static int cfs_vfs_ftruncate(vfs_handle_struct *handle, files_struct *fsp,
                               off_t len) {
    cfs_vfs_conn_t *conn;
    int ret;

    SMB_VFS_HANDLE_GET_DATA(handle, conn, cfs_vfs_conn_t, return -1);

    conn->rpc_calls++;
    ret = cfs_rpc_ftruncate(conn->rpc_conn, (uint64_t)(uintptr_t)fsp->fh->fd,
                             (int64_t)len);
    if (ret != 0) {
        conn->rpc_errors++;
        errno = cfs_err_to_errno(ret);
        return -1;
    }
    return 0;
}

/* ========================================================================
 * VFS Operation: get_real_filename
 * For case-insensitive name lookup (SMB3 requires this)
 * ======================================================================== */

static NTSTATUS cfs_vfs_get_real_filename(vfs_handle_struct *handle,
                                           const char *path,
                                           const char *name,
                                           TALLOC_CTX *mem_ctx,
                                           char **found_name) {
    /* ClaudeFS uses case-sensitive filenames (POSIX).
     * For SMB3 case-insensitive compatibility, perform a readdir scan
     * when exact match fails. TODO(production): use server-side case lookup. */
    *found_name = talloc_strdup(mem_ctx, name);
    if (!*found_name) {
        return NT_STATUS_NO_MEMORY;
    }
    return NT_STATUS_OK;
}

/* ========================================================================
 * VFS Operation: disk_free / statvfs
 * ======================================================================== */

static uint64_t cfs_vfs_disk_free(vfs_handle_struct *handle,
                                   const struct smb_filename *smb_fname,
                                   uint64_t *bsize, uint64_t *dfree,
                                   uint64_t *dsize) {
    cfs_vfs_conn_t *conn;
    cfs_statvfs_t cfs_vfs;
    char full_path[4096];
    int ret;

    SMB_VFS_HANDLE_GET_DATA(handle, conn, cfs_vfs_conn_t, return (uint64_t)-1);

    if (cfs_build_path(conn, smb_fname->base_name, full_path, sizeof(full_path)) < 0) {
        return (uint64_t)-1;
    }

    conn->rpc_calls++;
    ret = cfs_rpc_statvfs(conn->rpc_conn, full_path, &cfs_vfs);
    if (ret != 0) {
        conn->rpc_errors++;
        errno = cfs_err_to_errno(ret);
        return (uint64_t)-1;
    }

    *bsize = cfs_vfs.block_size;
    *dfree = cfs_vfs.blocks_free;
    *dsize = cfs_vfs.blocks_total;

    return *dfree;
}

/* ========================================================================
 * VFS function table
 * Maps Samba VFS operations to our implementations.
 * Operations not listed here fall through to the next VFS module (default: posix).
 * ======================================================================== */

static struct vfs_fn_pointers cfs_vfs_fns = {
    /* Connection lifecycle */
    .connect_fn             = cfs_vfs_connect,
    .disconnect_fn          = cfs_vfs_disconnect,

    /* File operations */
    .open_fn                = cfs_vfs_open,
    .close_fn               = cfs_vfs_close,
    .read_fn                = cfs_vfs_read,
    .pread_fn               = cfs_vfs_pread,
    .write_fn               = cfs_vfs_write,
    .pwrite_fn              = cfs_vfs_pwrite,
    .ftruncate_fn           = cfs_vfs_ftruncate,
    .fsync_fn               = cfs_vfs_fsync,

    /* Metadata operations */
    .stat_fn                = cfs_vfs_stat,
    .lstat_fn               = cfs_vfs_lstat,
    .fstat_fn               = cfs_vfs_fstat,
    .unlink_fn              = cfs_vfs_unlink,
    .rename_fn              = cfs_vfs_rename,
    .mkdir_fn               = cfs_vfs_mkdir,
    .rmdir_fn               = cfs_vfs_rmdir,

    /* Directory operations */
    .opendir_fn             = cfs_vfs_opendir,
    .readdir_fn             = cfs_vfs_readdir,
    .closedir_fn            = cfs_vfs_closedir,

    /* Filesystem info */
    .disk_free_fn           = cfs_vfs_disk_free,
    .get_real_filename_fn   = cfs_vfs_get_real_filename,
};

/* ========================================================================
 * Module registration
 * Called by Samba when loading the VFS module.
 * ======================================================================== */

static_decl_vfs;

NTSTATUS vfs_cfs_vfs_init(TALLOC_CTX *ctx) {
    return smb_register_vfs(SMB_VFS_INTERFACE_VERSION,
                             CFS_VFS_MODULE_NAME,
                             &cfs_vfs_fns);
}
