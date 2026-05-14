#!/usr/bin/env bash
# scripts/host/06-setup-file-share.sh
# Ubuntu host folder shared to the Windows VM through SMB.

set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
# shellcheck source=../lib/common.sh
source "$REPO_ROOT/scripts/lib/common.sh"

: "${WINBRIDGE_SHARE_NAME:=winbridge}"
: "${WINBRIDGE_SHARE_DIR:=$HOME/WinbridgeShare}"
: "${WINBRIDGE_SHARE_HOST:=192.168.122.1}"
: "${WINBRIDGE_SHARE_USER:=${SUDO_USER:-$USER}}"
: "${WINBRIDGE_CRED_DIR:=$HOME/.config/winbridge}"
: "${WINBRIDGE_CRED_FILE:=$WINBRIDGE_CRED_DIR/credentials}"

SMB_CONF=/etc/samba/smb.conf
DRY_RUN=0

usage() {
    cat <<USAGE
사용법: $0 [--help|--dry-run] [--home]
Windows VM에서 접근할 Ubuntu 전용 공유 폴더를 설정합니다.

기본값:
  공유 이름:     $WINBRIDGE_SHARE_NAME
  호스트 폴더:   $WINBRIDGE_SHARE_DIR
  Windows 경로:  \\\\$WINBRIDGE_SHARE_HOST\\$WINBRIDGE_SHARE_NAME
  Samba 사용자:  $WINBRIDGE_SHARE_USER

옵션:
  --home          공유 대상을 ~/WinbridgeShare 대신 \$HOME 전체로 설정

필요 패키지:
  sudo apt install -y samba
USAGE
}

for arg in "$@"; do
    case "$arg" in
        --dry-run)
            DRY_RUN=1
            ;;
        --home)
            WINBRIDGE_SHARE_DIR="$HOME"
            ;;
        --help|-h)
            usage
            exit 0
            ;;
        *)
            log_error "알 수 없는 옵션: $arg"
            log_error "사용법: $0 [--help|--dry-run] [--home]"
            exit 2
            ;;
    esac
done

if [ "$DRY_RUN" -eq 0 ]; then
    require_cmd smbd "sudo apt install -y samba"
    require_cmd testparm "sudo apt install -y samba"
    require_cmd smbpasswd "sudo apt install -y samba"
    require_cmd openssl "sudo apt install -y openssl"
fi

if [ "$DRY_RUN" -eq 1 ]; then
    log_info "[dry-run] mkdir -p $WINBRIDGE_SHARE_DIR"
    log_info "[dry-run] Samba share [$WINBRIDGE_SHARE_NAME] path=$WINBRIDGE_SHARE_DIR user=$WINBRIDGE_SHARE_USER"
    log_info "[dry-run] Windows path: \\\\$WINBRIDGE_SHARE_HOST\\$WINBRIDGE_SHARE_NAME"
    exit 0
fi

if ! id "$WINBRIDGE_SHARE_USER" >/dev/null 2>&1; then
    log_error "Samba 사용자 '$WINBRIDGE_SHARE_USER'를 찾을 수 없습니다"
    exit 1
fi

mkdir -p "$WINBRIDGE_SHARE_DIR" "$WINBRIDGE_CRED_DIR"
chmod 700 "$WINBRIDGE_CRED_DIR"
if [ "$WINBRIDGE_SHARE_DIR" != "$HOME" ]; then
    chmod 755 "$WINBRIDGE_SHARE_DIR"
fi

if [ -f "$WINBRIDGE_CRED_FILE" ] && grep -q '^WINBRIDGE_SAMBA_PASSWORD=' "$WINBRIDGE_CRED_FILE"; then
    WINBRIDGE_SAMBA_PASSWORD=$(
        sed -n 's/^WINBRIDGE_SAMBA_PASSWORD=//p' "$WINBRIDGE_CRED_FILE" | tail -1
    )
else
    WINBRIDGE_SAMBA_PASSWORD=$(openssl rand -hex 16)
    {
        [ -f "$WINBRIDGE_CRED_FILE" ] || echo "# winbridge credentials, generated $(date -Iseconds)"
        echo "WINBRIDGE_SAMBA_PASSWORD=$WINBRIDGE_SAMBA_PASSWORD"
    } >> "$WINBRIDGE_CRED_FILE"
    chmod 600 "$WINBRIDGE_CRED_FILE"
    log_info "Samba 비밀번호 생성: $WINBRIDGE_CRED_FILE"
fi

printf '%s\n%s\n' "$WINBRIDGE_SAMBA_PASSWORD" "$WINBRIDGE_SAMBA_PASSWORD" |
    sudo smbpasswd -s -a "$WINBRIDGE_SHARE_USER" >/dev/null

tmp_conf=$(mktemp)
sudo awk '
    /^# winbridge-share:BEGIN$/ { skip = 1; next }
    /^# winbridge-share:END$/ { skip = 0; next }
    skip != 1 { print }
' "$SMB_CONF" > "$tmp_conf"
cat >> "$tmp_conf" <<EOF

# winbridge-share:BEGIN
[$WINBRIDGE_SHARE_NAME]
   path = $WINBRIDGE_SHARE_DIR
   browseable = yes
   read only = no
   guest ok = no
   valid users = $WINBRIDGE_SHARE_USER
   force user = $WINBRIDGE_SHARE_USER
   create mask = 0644
   directory mask = 0755
   hosts allow = 192.168.122. 127.
# winbridge-share:END
EOF
sudo install -m 644 "$tmp_conf" "$SMB_CONF"
rm -f "$tmp_conf"

sudo testparm -s "$SMB_CONF" >/dev/null
sudo systemctl enable --now smbd >/dev/null 2>&1 || sudo service smbd start
sudo systemctl restart smbd nmbd >/dev/null 2>&1 || {
    sudo service smbd restart
    sudo service nmbd restart 2>/dev/null || true
}

log_info "파일 공유 설정 완료"
log_info "  Ubuntu 폴더: $WINBRIDGE_SHARE_DIR"
log_info "  Windows 경로: \\\\$WINBRIDGE_SHARE_HOST\\$WINBRIDGE_SHARE_NAME"
log_info "  사용자: $WINBRIDGE_SHARE_USER"
log_info "  비밀번호: $WINBRIDGE_CRED_FILE 의 WINBRIDGE_SAMBA_PASSWORD"
