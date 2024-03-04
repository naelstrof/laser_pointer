# laser_pointer

An app designed to allow a tutor to point at things on a student's screen remotely. It's best paired with a streaming
app like discord.

## Features

Automatic NAT port forwarding, easily share IP and port to connect and start pointing at stuff.

Allows for multiple people to connect to the same student simultaneously, multiplayer pointing!

Color-coded cursors, so you can tell which cursor is which.

Direct ip connections (unsafe, users will know where you live!), it sends raw UDP packets without any proxying. (sorry!)

## Usage

### As a Student

Simply run laser_pointer by double-clicking on it, then share the address and port it provides to the tutor.

### As a Tutor

Get the student's laser_pointer ip and port. Then run the following:

```shell
laser_pointer.exe --address=192.168.1.123:12345
```
but instead of `192.168.1.123:12345` use the address and port that they give.

This should open a window that allows you to click to make a cursor appear on their screen.
