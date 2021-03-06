# ssh-keys

ssh-keys is ssh-agent/pageant implementation for windows

## Getting Started
Download and run [ssh-keys.exe](https://github.com/KoHcoJlb/ssh-keys/releases/latest)  
**To add key** you can use ssh-add from OpenSSH.  
**To remove key** open %AppData%/ssh-keys/config.toml and remove block with the corresponding key.

To copy public key to remote user's authorized_keys use command  
`ssh-keys.exe copy-id [-p <port>] <username@host> <key>`  
`<key>` is the name of the key previously added to ssh-keys.

![](https://raw.githubusercontent.com/KoHcoJlb/ssh-keys/examples/confirmation.png)

## Features
* Supports Pageant protocol (Putty, WinSCP)
* Supports OpenSSH
* Supports WSL1
* Confirmation for key operations
* Displays which application wants to use key
* RSA keys
* Permanent key storage
* ssh-copy-id utility

### Planned
* ECDSA keys
* Password protected keys
* Confirmation improvements (graceful confirmation period,  
  focus "Ok" if requesting application is active)
* GUI for managing keys
* Ability to remove keys via ssh-add
