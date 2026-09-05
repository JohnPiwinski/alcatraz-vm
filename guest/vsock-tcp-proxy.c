/* Fixed Vibe ingress: guest vsock port 3000 -> guest loopback TCP 3000. */
#include <arpa/inet.h>
#include <errno.h>
#include <linux/vm_sockets.h>
#include <netinet/in.h>
#include <net/if.h>
#include <stdio.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/ioctl.h>
#include <unistd.h>

static void bring_loopback_up(void) {
    int ctl = socket(AF_INET, SOCK_DGRAM, 0);
    if (ctl < 0) return;
    struct ifreq request = {0};
    strncpy(request.ifr_name, "lo", IFNAMSIZ - 1);
    if (ioctl(ctl, SIOCGIFFLAGS, &request) == 0) {
        request.ifr_flags |= IFF_UP | IFF_RUNNING;
        (void)ioctl(ctl, SIOCSIFFLAGS, &request);
    }
    close(ctl);
}

static int connect_http(void) {
    bring_loopback_up();
    int fd = socket(AF_INET, SOCK_STREAM, 0);
    if (fd < 0) return -1;
    struct sockaddr_in addr = { .sin_family = AF_INET, .sin_port = htons(3000) };
    inet_pton(AF_INET, "127.0.0.1", &addr.sin_addr);
    if (connect(fd, (struct sockaddr *)&addr, sizeof(addr)) < 0) { close(fd); return -1; }
    return fd;
}

static void forward_once(int guest) {
    puts("ALCATRAZ_VIBE_VSOCK_HTTP_ACCEPT"); fflush(stdout);
    int local = connect_http();
    if (local < 0) { perror("http connect"); close(guest); return; }
    char buffer[16384]; ssize_t n;
    size_t received = 0;
    while (received < sizeof(buffer)) {
        n = read(guest, buffer + received, sizeof(buffer) - received);
        if (n <= 0) { perror("vsock request read"); close(local); close(guest); return; }
        received += (size_t)n;
        if (received >= 4 && memmem(buffer, received, "\r\n\r\n", 4) != NULL) break;
    }
    if (write(local, buffer, received) < 0) { perror("http request write"); close(local); close(guest); return; }
    shutdown(local, SHUT_WR);
    while ((n = read(local, buffer, sizeof(buffer))) > 0) {
        if (write(guest, buffer, (size_t)n) < 0) break;
    }
    puts("ALCATRAZ_VIBE_VSOCK_HTTP_DONE"); fflush(stdout);
    close(local); close(guest);
}

int main(void) {
    int server = socket(AF_VSOCK, SOCK_STREAM, 0);
    if (server < 0) { perror("vsock socket"); return 1; }
    struct sockaddr_vm addr = { .svm_family = AF_VSOCK, .svm_cid = VMADDR_CID_ANY, .svm_port = 3000 };
    if (bind(server, (struct sockaddr *)&addr, sizeof(addr)) < 0 || listen(server, 4) < 0) { perror("vsock bind/listen"); return 1; }
    puts("ALCATRAZ_VIBE_VSOCK_HTTP_LISTENING port=3000"); fflush(stdout);
    for (;;) { int guest = accept(server, NULL, NULL); if (guest >= 0) forward_once(guest); }
}
