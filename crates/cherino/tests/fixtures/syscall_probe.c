/*
 * Syscall probe for the live seccomp boundary test
 * (`tests/seccomp_boundary.rs`).
 *
 * One line per attempt: `<name>=<errno>` where 0 means the syscall completed.
 * The assertions live in the Rust test; this file only reports. Each probe is
 * chosen so that the *unfiltered* outcome is distinguishable from a seccomp
 * refusal (EPERM = 1), otherwise a missing capability or AppArmor rule would
 * look like a filter hit:
 *
 *   getpid            always succeeds (sanity: the harness runs at all)
 *   mount             succeeds with CAP_SYS_ADMIN, EPERM under a filter
 *   umount2           succeeds right after that mount
 *   unshare(CLONE_NEWNS)  succeeds with CAP_SYS_ADMIN, EPERM under a filter
 *   ptrace(TRACEME)   succeeds on an untraced process, EPERM under a filter
 *   bpf(0,0,0)        EINVAL (22) with a bad command, EPERM under a filter
 *   process_vm_readv  EINVAL (22) with zero lengths, EPERM under a filter
 *   fsopen            a mount-context fd for "tmpfs" when the new mount API is
 *                     reachable, EPERM under a filter (AppArmor does not mediate
 *                     the new API on kernel 6.1, so it must stay denied)
 *   open_tree         a cloned mount fd for "/" when reachable, EPERM under a
 *                     filter (same reason: clone + move_mount would mount
 *                     anything AppArmor's classic rules refuse)
 *
 * Build: gcc -O1 -Wall -o probe syscall_probe.c
 */
#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <sched.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mount.h>
#include <sys/ptrace.h>
#include <sys/stat.h>
#include <sys/syscall.h>
#include <sys/uio.h>
#include <unistd.h>

static void report(const char *name, long result, int err) {
    if (result == 0) {
        printf("%s=0\n", name);
    } else {
        printf("%s=%d\n", name, err);
    }
}

static void probe(const char *name, long (*fn)(void)) {
    errno = 0;
    long result = fn();
    report(name, result, errno);
}

static long p_getpid(void) { return (long)getpid(); }

static long p_mount(void) {
    const char *target = "/tmp/syscall-probe-mnt";
    mkdir(target, 0700);
    return mount("none", target, "tmpfs", 0, "size=4096");
}

static long p_umount2(void) { return umount2("/tmp/syscall-probe-mnt", 0); }

static long p_unshare(void) { return unshare(CLONE_NEWNS); }

static long p_ptrace(void) { return ptrace(PTRACE_TRACEME, 0, 0, 0); }

static long p_bpf(void) { return syscall(SYS_bpf, 0, 0, 0); }

#ifndef SYS_fsopen
#define SYS_fsopen 430
#endif
#ifndef SYS_open_tree
#define SYS_open_tree 428
#endif
#define OPEN_TREE_CLONE 1
#ifndef AT_RECURSIVE
#define AT_RECURSIVE 0x8000
#endif

static long p_fsopen(void) { return syscall(SYS_fsopen, "tmpfs", 0); }

static long p_open_tree(void) {
    return syscall(SYS_open_tree, AT_FDCWD, "/", OPEN_TREE_CLONE | AT_RECURSIVE);
}

static long p_process_vm_readv(void) {
    struct iovec local = {0};
    struct iovec remote = {0};
    return syscall(SYS_process_vm_readv, getpid(), &local, 0, &remote, 0, 0);
}

int main(void) {
    probe("getpid", p_getpid);
    /* The mount/umount pair runs first: `unshare` below moves this process into
     * a new mount namespace, and a mount made afterwards would not be visible
     * to the parent, which is fine but harder to reason about. */
    probe("mount", p_mount);
    probe("umount2", p_umount2);
    probe("unshare", p_unshare);
    probe("ptrace", p_ptrace);
    probe("bpf", p_bpf);
    probe("process_vm_readv", p_process_vm_readv);
    /* The new mount API is the AppArmor bypass this profile must not open:
     * neither syscall is AppArmor-mediated on kernel 6.1. */
    probe("fsopen", p_fsopen);
    probe("open_tree", p_open_tree);
    return 0;
}
