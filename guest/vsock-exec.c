/* Fixed guest service: one Firecracker vsock connection -> one allowlisted argv. */
#include <errno.h>
#include <linux/vm_sockets.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <sys/socket.h>
#include <sys/wait.h>
#include <unistd.h>

int main(int argc, char **argv) {
    if (argc != 3) { fprintf(stderr, "usage: vsock-exec PORT EXECUTABLE\n"); return 2; }
    char *end = NULL; long port = strtol(argv[1], &end, 10);
    if (*argv[1] == 0 || *end != 0 || port < 1024 || port > 65535) return 2;
    int server = socket(AF_VSOCK, SOCK_STREAM, 0);
    if (server < 0) { perror("vsock socket"); return 1; }
    struct sockaddr_vm addr = { .svm_family = AF_VSOCK, .svm_cid = VMADDR_CID_ANY, .svm_port = (unsigned int)port };
    if (bind(server, (struct sockaddr *)&addr, sizeof(addr)) < 0 || listen(server, 4) < 0) { perror("vsock bind/listen"); return 1; }
    printf("ALCATRAZ_VSOCK_EXEC_LISTENING port=%ld\n", port); fflush(stdout);
    for (;;) {
        int peer = accept(server, NULL, NULL);
        if (peer < 0) { if (errno == EINTR) continue; return 1; }
        pid_t child = fork();
        if (child == 0) {
            close(server); dup2(peer, STDIN_FILENO); dup2(peer, STDOUT_FILENO); dup2(peer, STDERR_FILENO); close(peer);
            execl(argv[2], argv[2], (char *)NULL); _exit(127);
        }
        close(peer);
        waitpid(child, NULL, 0);
    }
}
