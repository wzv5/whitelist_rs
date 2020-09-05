#!/bin/sh
sudo systemctl stop whitelist_rs
sudo systemctl disable whitelist_rs
workdir=$(cd $(dirname $0); pwd)
sudo cp $workdir/whitelist_rs.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable whitelist_rs
sudo systemctl start whitelist_rs
