#!/bin/sh
sed -i 's/^pathhost:.*/pathhost: news.example.com/' /etc/news/inn.conf
grep -q '^domain' /etc/news/inn.conf || echo 'domain: example.com' >> /etc/news/inn.conf
printf 'peer ME {\n    hostname: "localhost"\n}\n' > /etc/news/incoming.conf
mkdir -p /run/news /var/run/news; chown -R news:news /var/lib/news /var/spool/news /var/log/news /run/news 2>/dev/null
su news -s /bin/sh -c "/usr/lib/news/bin/rc.news" >/tmp/rc.log 2>&1
sleep 3
su news -s /bin/sh -c "/usr/lib/news/bin/ctlinnd newgroup alt.binaries.test y local" >/tmp/ctl.log 2>&1
cat /tmp/rc.log /tmp/ctl.log
exec tail -f /var/log/news/news /var/log/news/news.notice 2>/dev/null
