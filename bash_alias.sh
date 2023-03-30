#!/bin/sh

# This file provides command alias commonly used by the developers involved in Canyon-SQL 
# This alias avoid the usage of a bunch of commands for performn an integrated task that 
# depends on several concatenated commands.

# In order to run the script, simply type `$ . ./alias.sh` from the root of the project.
# (refreshing the current terminal session could be required)

# Executes the docker compose script to wake up the postgres container
alias DockerUp='docker-compose -f ./docker/docker-compose.yml up'
# Shutdown the postgres container
alias DockerDown='docker-compose -f ./docker/docker-compose.yml down'
# Cleans the generated cache folder for the postgres in the docker
alias CleanPostgres='rm -rf ./docker/postgres-data'